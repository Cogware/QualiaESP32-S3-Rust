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
use lvgl::NativeObject;
use lvgl::{Align, Color, Display, DrawBuffer, Part, TextAlign, Widget};
use lvgl_sys::lv_event_code_t_LV_EVENT_ALL;
use lvgl_sys::lv_font_montserrat_48;
use lvgl_sys::lv_meter_add_needle_line;
use lvgl_sys::lv_meter_set_indicator_end_value;
use lvgl_sys::lv_meter_set_indicator_start_value;
use lvgl_sys::lv_meter_set_indicator_value;
use lvgl_sys::lv_meter_set_scale_major_ticks;
use lvgl_sys::lv_obj_add_event_cb;
use lvgl_sys::lv_obj_set_style_text_color;
use lvgl_sys::lv_obj_set_style_text_font;
use lvgl_sys::LV_PART_TICKS;
use lvgl_sys::_LV_COLOR_MAKE;
use port_expander::dev::pca9554::Pca9554A;
use qualia_hello_world::display::*;
use qualia_hello_world::meter_draw_event_cb;
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
    //let (front, back) = lcd_panel.get_buffers(); //something here? wanted to directly write to buffers from LVGL and just memswap instead of fancy writes
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
        },
    )
    .unwrap();

    log::info!("Display inited");

    let mut screen = display.get_scr_act().unwrap();
    let mut screen_style = Style::default();
    screen_style.set_bg_color(Color::from_rgb((0, 0, 0)));
    screen_style.set_radius(0);
    screen.add_style(Part::Main, &mut screen_style).unwrap();

    let mut gauge = lvgl::widgets::Meter::create(&mut screen).unwrap();
    gauge.set_size(720, 720).unwrap();
    gauge.set_pos(0, 0).unwrap();
    let gaugeraw = gauge.raw().unwrap().as_ptr();
    gauge
        .add_style(lvgl::Part::Main, &mut screen_style)
        .unwrap();
    let scale = unsafe { lvgl_sys::lv_meter_add_scale(gaugeraw) };

    let maincolor = unsafe { _LV_COLOR_MAKE(240, 130, 0) };
    unsafe {
        lv_obj_add_event_cb(
            gaugeraw,
            Some(meter_draw_event_cb),
            lv_event_code_t_LV_EVENT_ALL,
            core::ptr::null_mut(),
        );
        let redline =
            lvgl_sys::lv_meter_add_arc(gaugeraw, scale, 10, _LV_COLOR_MAKE(255, 0, 0), -50);
        let shiftlow =
            lvgl_sys::lv_meter_add_arc(gaugeraw, scale, 10, _LV_COLOR_MAKE(0, 255, 0), -50);
        let shifthigh =
            lvgl_sys::lv_meter_add_arc(gaugeraw, scale, 10, _LV_COLOR_MAKE(255, 255, 0), -50);
        lvgl_sys::lv_meter_set_scale_range(gaugeraw, scale, 0, 8000, 240, 150);
        lvgl_sys::lv_meter_set_scale_ticks(gaugeraw, scale, 33, 5, 20, maincolor);
        lv_meter_set_scale_major_ticks(gaugeraw, scale, 4, 10, 40, maincolor, 50);
        lv_meter_set_indicator_start_value(gaugeraw, redline, 5500);
        lv_meter_set_indicator_end_value(gaugeraw, redline, 8000);
        lv_obj_set_style_text_color(gaugeraw, maincolor, LV_PART_TICKS);
        lv_obj_set_style_text_font(gaugeraw, &lv_font_montserrat_48, LV_PART_TICKS);
        lv_meter_set_indicator_start_value(gaugeraw, shiftlow, 2000);
        lv_meter_set_indicator_end_value(gaugeraw, shiftlow, 2500);
        lv_meter_set_indicator_start_value(gaugeraw, shifthigh, 4000);
        lv_meter_set_indicator_end_value(gaugeraw, shifthigh, 4500);
    }
    let needle = unsafe { lv_meter_add_needle_line(gaugeraw, scale, 7, maincolor, -45) };

    let mut tach: u16 = 0;
    loop {
        let start = now();
        tach = tach.wrapping_add(100);
        if tach > 8000 {
            tach = 0;
        }
        unsafe {
            lv_meter_set_indicator_value(gaugeraw, needle, tach.into());
        }
        //log::info!("task_handler");
        lvgl::task_handler();
        //log::info!("frame?"); //prints litterally crash it because the watchdog freaks out
        FreeRtos::delay_ms(5);
        lvgl::tick_inc(now() - start);
    }
}
