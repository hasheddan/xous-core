#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

use com::*;
use core::fmt::Write;
use graphics_server::{Point, Rectangle, PixelColor, DrawStyle, Line, Circle};
use blitstr::{Cursor, GlyphStyle};
use log::{error, info};
use xous::String;

use core::convert::TryFrom;

fn move_lfsr(mut lfsr: u32) -> u32 {
    lfsr ^= lfsr >> 7;
    lfsr ^= lfsr << 9;
    lfsr ^= lfsr >> 13;
    lfsr
}

pub struct Bounce {
    vector: Point,
    radius: u16,
    bounds: Rectangle,
    loc: Point,
    lfsr: u32,
}

impl Bounce {
    pub fn new(radius: u16, bounds: Rectangle) -> Bounce {
        Bounce {
            vector: Point::new(2, 3),
            radius: radius,
            bounds: bounds,
            loc: Point::new(
                (bounds.br.x - bounds.tl.x) / 2,
                (bounds.br.y - bounds.tl.y) / 2,
            ),
            lfsr: 0xace1u32,
        }
    }

    pub fn ball_center(&self) -> Point {
        self.loc
    }
    pub fn radius(&self) -> u16 {
        self.radius
    }
    pub fn bounds(&self) -> Rectangle {
        self.bounds
    }

    pub fn next_rand(&mut self) -> i16 {
        let mut ret = move_lfsr(self.lfsr);
        self.lfsr = ret;
        ret *= 3; // make the ball move faster

        (ret % 12) as i16
    }

    pub fn update(&mut self) -> &mut Self {
        let mut x: i16;
        let mut y: i16;
        // update the new ball location
        x = self.loc.x + self.vector.x;
        y = self.loc.y + self.vector.y;

        let r: i16 = self.radius as i16;
        if (x >= (self.bounds.br.x - r))
            || (x <= (self.bounds.tl.x + r))
            || (y >= (self.bounds.br.y - r))
            || (y <= (self.bounds.tl.y + r))
        {
            if x >= (self.bounds.br.x - r - 1) {
                self.vector.x = -self.next_rand();
                x = self.bounds.br.x - r;
            }
            if x <= self.bounds.tl.x + r + 1 {
                self.vector.x = self.next_rand();
                x = self.bounds.tl.x + r;
            }
            if y >= (self.bounds.br.y - r - 1) {
                self.vector.y = -self.next_rand();
                y = self.bounds.br.y - r;
            }
            if y <= (self.bounds.tl.y + r + 1) {
                self.vector.y = self.next_rand();
                y = self.bounds.tl.y + r;
            }
        }

        self.loc.x = x;
        self.loc.y = y;

        self
    }
}

use core::sync::atomic::{AtomicI16, AtomicU16, AtomicU8, Ordering};

// need atomic global constants to pass data between threads
// as we do not yet have a "Mutex" in Xous
static BATT_STATS_VOLTAGE: AtomicU16 = AtomicU16::new(3700);
static BATT_STATS_CURRENT: AtomicI16 = AtomicI16::new(-150);
static BATT_STATS_SOC: AtomicU8 = AtomicU8::new(50);
static BATT_STATS_REMAINING: AtomicU16 = AtomicU16::new(750);

fn com_thread(_arg: Option<u32>) {
    let shell_server =
        xous::create_server_with_address(b"shell           ").expect("Couldn't create Shell server");
    info!("SHELL|com_thread: starting COM response handler thread");
    loop {
        let envelope =
            xous::syscall::receive_message(shell_server).expect("couldn't get address");
        info!("SHELL|com_thread: got message {:?}", envelope);
        if let Ok(opcode) = com::api::Opcode::try_from(&envelope.body) {
            match opcode {
                com::api::Opcode::BattStatsReturn(stats) => {
                    BATT_STATS_VOLTAGE.store(stats.voltage, Ordering::Relaxed);
                    BATT_STATS_CURRENT.store(stats.current, Ordering::Relaxed);
                    BATT_STATS_SOC.store(stats.soc, Ordering::Relaxed);
                    BATT_STATS_REMAINING.store(stats.remaining_capacity, Ordering::Relaxed);
                }
                _ => error!("shell received an opcode that wasn't expected"),
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();

    // let log_server_id = xous::SID::from_bytes(b"xous-logs-output").unwrap();
    let graphics_server_id = xous::SID::from_bytes(b"graphics-server ").unwrap();
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let log_server_id = xous::SID::from_bytes(b"xous-log-server ").unwrap();
    let com_id = xous::SID::from_bytes(b"com             ").unwrap();

    let log_conn = xous::connect(log_server_id).unwrap();
    let graphics_conn = xous::connect(graphics_server_id).unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    let com_conn = xous::connect(com_id).unwrap();

    info!(
        "SHELL: Connected to Log server: {}  Graphics server: {}  Ticktimer server: {} Com: {}",
        log_conn, graphics_conn, ticktimer_conn, com_conn,
    );

    assert_ne!(
        log_conn, graphics_conn,
        "SHELL: graphics and log connections are the same!"
    );

    assert_ne!(
        ticktimer_conn, graphics_conn,
        "SHELL: graphics and ticktimer connections are the same!"
    );

    // make a thread to catch responses from the COM
    xous::create_thread_simple(com_thread, None).unwrap();
    info!("SHELL: COM responder thread started");

    let screensize = graphics_server::screen_size(graphics_conn).expect("Couldn't get screen size");

    let mut bouncyball = Bounce::new(
        14,
        Rectangle::new(
            Point::new(0, 18 * 21),
            Point::new(screensize.x as _, screensize.y as i16 - 1),
        ),
    );
    bouncyball.update();

    #[cfg(baremetal)]
    {
        // use this to select which UART to monitor in the main loop
        use utralib::generated::*;
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        let mut gpio = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
        gpio.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 = kernel, 1 = log, 2-3 are various servers
    }

    let style_dark = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
    let style_light = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);

    let mut last_time: u64 = 0;
    ticktimer_server::reset(ticktimer_conn).unwrap();
    let mut string_buffer = String::new(4096);
    graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Small).expect("unable to set glyph");
    let (_, font_h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
    let status_clipregion = Rectangle::new_coords_with_style(4, 0, screensize.x, font_h as _, style_light);
    let mut status_cursor;

    graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Regular).expect("unable to set glyph");
    let (_, font_h) = graphics_server::query_glyph(graphics_conn).expect("unable to query glyph");
    let mut work_clipregion = Rectangle::new_coords_with_style(4, font_h as i16 + 2, screensize.x, font_h as i16 * 8 + 18, style_light);
    let mut work_cursor;
    graphics_server::draw_rectangle(graphics_conn, work_clipregion)
            .expect("unable to clear region");

    let mut firsttime = true;
    loop {
        // status bar
        graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Small).expect("unable to set glyph");

        graphics_server::draw_rectangle(graphics_conn, status_clipregion)
            .expect("unable to clear region");
        graphics_server::set_string_clipping(graphics_conn, status_clipregion.into())
            .expect("unable to set string clip region");
        string_buffer.clear();
        write!(&mut string_buffer, "{}mV", BATT_STATS_VOLTAGE.load(Ordering::Relaxed)).expect("Can't write");
        status_cursor = Cursor::from_top_left_of(status_clipregion.into());
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        status_cursor.pt.x = 95;
        string_buffer.clear();
        write!(&mut string_buffer, "{}mA", BATT_STATS_CURRENT.load(Ordering::Relaxed)).expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        status_cursor.pt.x = 190;
        string_buffer.clear();
        write!(&mut string_buffer, "{}mA", BATT_STATS_REMAINING.load(Ordering::Relaxed)).expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
        status_cursor.pt.x = 280;
        string_buffer.clear();
        write!(&mut string_buffer, "{}%", BATT_STATS_SOC.load(Ordering::Relaxed)).expect("Can't write");
        graphics_server::set_cursor(graphics_conn, status_cursor).expect("can't set cursor");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");

        graphics_server::draw_line(graphics_conn, Line::new_with_style(
            Point::new(0, font_h as i16),
            Point::new(screensize.x as _, font_h as i16),
            style_dark)).expect("can't draw line");

        // work area
        string_buffer.clear();
        write!(&mut string_buffer,
            "Uptime: {:.2}s\n\n", last_time as f32 / 1000f32
        ).expect("Can't write");
        work_cursor = Cursor::from_top_left_of(work_clipregion.into());
        work_clipregion.br = Point::new(screensize.x, font_h as i16 * 3);
        graphics_server::draw_rectangle(graphics_conn, work_clipregion)
            .expect("unable to clear region");
            work_clipregion.br = Point::new(screensize.x, font_h as i16 * 8);
            graphics_server::set_string_clipping(graphics_conn, work_clipregion.into())
            .expect("unable to set string clip region");
        graphics_server::set_cursor(graphics_conn, work_cursor).expect("can't set cursor");
        graphics_server::set_glyph_style(graphics_conn, GlyphStyle::Regular).expect("unable to set glyph");
        graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");

        if firsttime {
            string_buffer.clear();
            write!(&mut string_buffer, "Zwölf Boxkämpfer jagen Viktor quer über den großen Sylter Deich.\n           😸     🎩    🔑\n           cat    hat    key\n").expect("Can't write");
            graphics_server::draw_string(graphics_conn, &string_buffer).expect("unable to draw string");
            firsttime = false;
        }

        // ticktimer_server::sleep_ms(ticktimer_conn, 500).expect("couldn't sleep");

        // draw the ball
        graphics_server::draw_rectangle(
            graphics_conn,
            Rectangle::new_with_style(
                Point::new(
                    bouncyball.ball_center().x - bouncyball.radius() as i16 - 1,
                    bouncyball.ball_center().y - bouncyball.radius() as i16 - 1),
                Point::new(
                    bouncyball.ball_center().x + bouncyball.radius() as i16 + 1,
                    bouncyball.ball_center().y + bouncyball.radius() as i16 + 1),
                style_light
            )
        )
        .expect("unable to clear ball region");
        bouncyball.update();

        // draw the top line that contains the ball
        graphics_server::draw_line(graphics_conn,
       Line::new_with_style(Point::new(0, bouncyball.bounds.tl.y - 1),
            Point::new(screensize.x, bouncyball.bounds.tl.y - 1), style_dark)).expect("can't draw border");
        // draw the ball
        graphics_server::draw_circle(graphics_conn,
        Circle::new_with_style(bouncyball.loc, bouncyball.radius as i16, style_dark))
            .expect("unable to draw to screen");

        // Periodic tasks
        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 500 {
                last_time = elapsed_time;
                info!("Requesting batt stats from COM");
                get_batt_stats_nb(com_conn).expect("Can't get battery stats from COM");
            }
        } else {
            error!("error requesting ticktimer!")
        }

        graphics_server::flush(graphics_conn).expect("unable to draw to screen");
    }
}
