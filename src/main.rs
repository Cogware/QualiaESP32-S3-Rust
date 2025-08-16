use std::os::raw::c_void;

use cstr_core::CString;
use esp_idf_hal::delay;
use esp_idf_hal::i2c::*;
use esp_idf_hal::prelude::*;
use esp_idf_svc::hal::{delay::FreeRtos, prelude::Peripherals};
use esp_idf_sys::*;
use lvgl::font::Font;
use lvgl::style::Style;
use lvgl::widgets::Label;
use lvgl::{Align, Color, Display, DrawBuffer, Part, TextAlign, Widget};
use port_expander::dev::pca9554::Pca9554A;
use qualia_hello_world::display::*;
use qualia_hello_world::now;

const HOR_RES: usize = 720;
const VER_RES: usize = 720;
const LINES: usize = 20;
fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Start!");

    let mut draw_buf: [lvgl::Color; HOR_RES * LINES] =
        [lvgl::Color::from_rgb((0, 0, 0)); HOR_RES * LINES];

    unsafe {
        let cfg = esp_pm_config_t {
            max_freq_mhz: 240,
            min_freq_mhz: 240,
            light_sleep_enable: false,
        };
        esp!(esp_pm_configure(
            (&cfg as *const esp_pm_config_t) as *const core::ffi::c_void
        ))
        .unwrap();
    }

    let p = Peripherals::take().unwrap();
    let i2c = p.i2c0;
    let sda = p.pins.gpio8;
    let scl = p.pins.gpio18;
    let config = I2cConfig::new().baudrate(400.kHz().into());
    let i2c = I2cDriver::new(i2c, sda, scl, &config).unwrap();

    let mut p = Pca9554A::new(i2c, true, true, true);
    let pca_pins = p.split();

    let tft_sck = pca_pins.io0.into_output().unwrap();
    let tft_cs = pca_pins.io1.into_output().unwrap();
    let mut tft_reset = pca_pins.io2.into_output().unwrap();
    let _tp_irq = pca_pins.io3.into_output().unwrap();
    let mut backlight = pca_pins.io4.into_output().unwrap();
    let btn_up = pca_pins.io5.into_input().unwrap();
    let btn_down = pca_pins.io6.into_input().unwrap();
    let tft_mosi = pca_pins.io7.into_output().unwrap();
    log::info!("pins inited");

    backlight.set_high().ok();

    tft_reset.set_high().ok();
    FreeRtos::delay_ms(20);
    tft_reset.set_low().ok();
    FreeRtos::delay_ms(20); // hold reset low long enough (datasheet: ~10–50 ms)
    tft_reset.set_high().ok();
    FreeRtos::delay_ms(20);

    lvgl::init();

    let mut lcd_panel = LcdPanel::new(
        &PanelConfig::new(),
        &PanelFlagsConfig::new(),
        &TimingsConfig::new(),
        &TimingFlagsConfig::new(),
    )
    .unwrap();

    log::info!("=============  Registering Display ====================");
    let buffer = DrawBuffer::<{ (HOR_RES * LINES) as usize }>::default();
    let display = Display::register(
        buffer,
        HOR_RES.try_into().unwrap(),
        VER_RES.try_into().unwrap(),
        |refresh| {
            unsafe {
                esp!(esp_lcd_panel_draw_bitmap(
                    lcd_panel.ret_mut_dref(),
                    refresh.area.x1.into(),
                    refresh.area.y1.into(),
                    (refresh.area.x2 + 1i16).into(),
                    (refresh.area.y2 + 1i16).into(),
                    refresh.colors.as_ptr() as *const c_void
                ))
                .unwrap()
            };
            /*lcd_panel
            .set_pixels_lvgl_color(
                refresh.area.x1.into(),
                refresh.area.y1.into(),
                (refresh.area.x2 + 1i16).into(),
                (refresh.area.y2 + 1i16).into(),
                refresh.colors.as_ptr(),
            )
            .unwrap();*/
        },
    )
    .unwrap();

    log::info!("Display inited");

    let mut screen = display.get_scr_act().unwrap();
    let mut screen_style = Style::default();
    screen_style.set_bg_color(Color::from_rgb((0, 0, 255)));
    screen_style.set_radius(0);
    screen.add_style(Part::Main, &mut screen_style).unwrap();

    let mut time = Label::new().unwrap();
    let mut style_time = Style::default();
    style_time.set_text_color(Color::from_rgb((255, 255, 255))); // white
    style_time.set_text_align(TextAlign::Center);

    // Custom font requires lvgl-sys in Cargo.toml and 'use lvgl_sys' in this file
    style_time.set_text_font(unsafe { Font::new_raw(lvgl_sys::lv_font_montserrat_14) });

    time.add_style(Part::Main, &mut style_time).unwrap();

    // Time text will be centered in screen
    time.set_align(Align::Center, 0, 0).unwrap();

    let mut i = 0;
    loop {
        let start = now();
        if i > 59 {
            i = 0;
        }

        let val = CString::new(format!("21:{:02}", i)).unwrap();
        time.set_text(&val).unwrap();
        i += 1;
        //log::info!("task_handler");
        lvgl::task_handler();
        //log::info!("frame?"); //prints litterally crash it because the watchdog freaks out
        // Simulate clock - so sleep for one second so time text is incremented in seconds
        delay::FreeRtos::delay_ms(1000);

        lvgl::tick_inc(now() - start);
    }
}
