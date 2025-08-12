use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyle},
    text::{Alignment, Text},
};
use esp_idf_hal::i2c::*;
use esp_idf_hal::prelude::*;
use esp_idf_svc::hal::{delay::FreeRtos, prelude::Peripherals};
use esp_idf_sys::*;
use port_expander::dev::pca9554::Pca9554A;
use qualia_hello_world::ExpanderSpi;
use qualia_hello_world::LcdFbs;
use qualia_hello_world::{display_task, init_display, render_task};

static W: u16 = 480;
static H: u16 = 480;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Start!");

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
    let _backlight = pca_pins.io4.into_output().unwrap();
    let btn_up = pca_pins.io5.into_input().unwrap();
    let btn_down = pca_pins.io6.into_input().unwrap();
    let tft_mosi = pca_pins.io7.into_output().unwrap();
    log::info!("pins inited");

    tft_reset.set_high().ok();
    FreeRtos::delay_ms(10);
    tft_reset.set_low().ok();
    FreeRtos::delay_ms(10); // hold reset low long enough (datasheet: ~10–50 ms)
    tft_reset.set_high().ok();
    FreeRtos::delay_ms(10);

    let mut fakespi = ExpanderSpi::new(tft_sck, tft_cs, tft_mosi, 1).unwrap();
    init_display(&mut fakespi).unwrap();

    let mut panel_handle: esp_lcd_panel_handle_t = ptr::null_mut();

    unsafe {
        let mut timings = esp_lcd_rgb_timing_t {
            pclk_hz: 8_000_000,
            h_res: W.into(),
            v_res: H.into(),
            hsync_front_porch: 40,
            hsync_pulse_width: 20,
            hsync_back_porch: 40,
            vsync_front_porch: 40,
            vsync_pulse_width: 10,
            vsync_back_porch: 40,
            ..Default::default()
        };
        timings.flags.set_pclk_active_neg(1);

        let mut config = esp_lcd_rgb_panel_config_t {
            clk_src: soc_periph_lcd_clk_src_t_LCD_CLK_SRC_DEFAULT,
            timings,
            data_width: 16,
            bits_per_pixel: 16,
            num_fbs: 2,
            bounce_buffer_size_px: 0,
            sram_trans_align: 64,
            psram_trans_align: 64,
            hsync_gpio_num: 41,
            vsync_gpio_num: 42,
            de_gpio_num: 2,
            pclk_gpio_num: 1,
            disp_gpio_num: -1,
            data_gpio_nums: [11, 10, 9, 46, 3, 48, 47, 21, 14, 13, 12, 40, 39, 38, 0, 45],
            ..Default::default()
        };

        config.flags.set_fb_in_psram(1);
        config.flags.set_double_fb(1);

        let ret = esp_lcd_new_rgb_panel(&config as *const _, &mut panel_handle as *mut _);
        if ret != ESP_OK {
            esp_rom_printf(b"esp_lcd_new_rgb_panel failed: %d\r\n\0".as_ptr() as _, ret);
            return;
        }

        esp_lcd_panel_reset(panel_handle);

        esp_lcd_panel_init(panel_handle);

        esp_lcd_panel_disp_on_off(panel_handle, true);
    }
    log::info!("Display inited");

    let mut fb = unsafe { LcdFbs::new(panel_handle, W.into(), H.into()) };

    unsafe {
        let fbs: &'static mut LcdFbs<'static> =
            core::mem::transmute::<_, _>(Box::leak(Box::new(LcdFbs::new(panel_handle, 480, 480))));

        // display on core 0
        xTaskCreatePinnedToCore(
            Some(display_task_trampoline),
            b"display\0".as_ptr() as *const u8,
            6144,
            fbs as *mut _ as *mut _,
            5,
            core::ptr::null_mut(),
            0,
        );

        // renderer on core 1
        xTaskCreatePinnedToCore(
            Some(render_task_trampoline),
            b"render\0".as_ptr() as *const u8,
            8192,
            fbs as *mut _ as *mut _,
            4,
            core::ptr::null_mut(),
            1,
        );
    }
}

extern "C" fn display_task_trampoline(arg: *mut core::ffi::c_void) {
    unsafe { display_task(arg as *mut LcdFbs) }
}

extern "C" fn render_task_trampoline(arg: *mut core::ffi::c_void) {
    unsafe { render_task(arg as *mut LcdFbs) }
}
