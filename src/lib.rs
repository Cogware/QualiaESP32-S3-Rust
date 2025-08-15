#![no_std]

use core::convert::Infallible;
use core::slice;

use core::ffi::c_void;
use core::marker::PhantomData;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Angle;
use embedded_graphics::prelude::Dimensions;
use embedded_graphics::prelude::DrawTarget;
use embedded_graphics::prelude::IntoStorage;
use embedded_graphics::prelude::OriginDimensions;
use embedded_graphics::prelude::Point;
use embedded_graphics::prelude::Primitive;
use embedded_graphics::prelude::RgbColor;
use embedded_graphics::prelude::Size;
use embedded_graphics::prelude::WebColors;
use embedded_graphics::primitives::Arc;
use embedded_graphics::primitives::Circle;
use embedded_graphics::primitives::PrimitiveStyle;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::Alignment;
use embedded_graphics::text::Text;
use embedded_graphics::Drawable;
use embedded_graphics::Pixel;
use embedded_hal::digital::{ErrorType, OutputPin};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_svc::sys::esp;
use esp_idf_sys::esp_lcd_rgb_panel_event_callbacks_t;
use esp_idf_sys::esp_lcd_rgb_panel_event_data_t;
use esp_idf_sys::esp_lcd_rgb_panel_refresh;
use esp_idf_sys::esp_lcd_rgb_panel_register_event_callbacks;
use esp_idf_sys::BaseType_t;
use esp_idf_sys::QueueHandle_t;
use esp_idf_sys::{
    esp_lcd_panel_draw_bitmap, esp_lcd_panel_handle_t, esp_lcd_rgb_panel_get_frame_buffer,
};

/// Bit-banged SPI over port-expander pins (mode 0, MSB-first).
pub struct ExpanderSpi<SCK, CS, MOSI>
where
    SCK: OutputPin,
    CS: OutputPin<Error = <SCK as ErrorType>::Error>,
    MOSI: OutputPin<Error = <SCK as ErrorType>::Error>,
{
    sck: SCK,
    cs: CS,
    mosi: MOSI,
    half_period_us: u32,
}

impl<SCK, CS, MOSI> ErrorType for ExpanderSpi<SCK, CS, MOSI>
where
    SCK: OutputPin,
    CS: OutputPin<Error = <SCK as ErrorType>::Error>,
    MOSI: OutputPin<Error = <SCK as ErrorType>::Error>,
{
    type Error = <SCK as ErrorType>::Error;
}

impl<SCK, CS, MOSI> ExpanderSpi<SCK, CS, MOSI>
where
    SCK: OutputPin,
    CS: OutputPin<Error = <SCK as ErrorType>::Error>,
    MOSI: OutputPin<Error = <SCK as ErrorType>::Error>,
{
    /// `half_period_us`: delay between edges; 0 = as fast as calls allow.
    pub fn new(
        mut sck: SCK,
        mut cs: CS,
        mut mosi: MOSI,
        half_period_us: u32,
    ) -> Result<Self, <Self as ErrorType>::Error> {
        // Idle state: SCK=0, CS=1, MOSI=0
        sck.set_low()?;
        cs.set_high()?;
        mosi.set_low()?;
        Ok(Self {
            sck,
            cs,
            mosi,
            half_period_us,
        })
    }

    #[inline]
    pub fn set_half_period_us(&mut self, hp: u32) {
        self.half_period_us = hp;
    }

    #[inline]
    pub fn set_cs_low(&mut self) -> Result<(), <Self as ErrorType>::Error> {
        self.cs.set_low()
    }
    #[inline]
    pub fn set_cs_high(&mut self) -> Result<(), <Self as ErrorType>::Error> {
        self.cs.set_high()
    }

    /// Write `nbits` (1..=32), MSB-first. SPI mode-0: data valid on rising edge.
    pub fn write_bits(&mut self, value: u32, nbits: u8) -> Result<(), <Self as ErrorType>::Error> {
        debug_assert!(nbits >= 1 && nbits <= 32);
        for i in (0..nbits).rev() {
            let bit_is_one = ((value >> i) & 1) != 0;
            if bit_is_one {
                self.mosi.set_high()?;
            } else {
                self.mosi.set_low()?;
            }
            self.edge_delay();

            self.sck.set_high()?; // rising edge: target latches MOSI
            self.edge_delay();

            self.sck.set_low()?; // return low for mode-0
                                 // keep MOSI stable until next bit
        }
        self.mosi.set_low()?;
        Ok(())
    }

    /// 9-bit command write: sends raw 9 bits as provided.
    /// Convention: bit8 = D/C (0 = command, 1 = data).
    #[inline]
    pub fn write_command(&mut self, by: u32) -> Result<(), <Self as ErrorType>::Error> {
        self.set_cs_low()?;
        self.write_bits(by, 9)?;
        self.set_cs_high()
    }

    /// Helper that sends a command byte (D/C=0) followed by data bytes (D/C=1).
    #[inline]
    pub fn write_data(
        &mut self,
        reg: u32,
        bytes: &[u32],
    ) -> Result<(), <Self as ErrorType>::Error> {
        // Send command (D/C=0): top bit = 0, payload = reg[7:0]
        self.write_command(reg & 0x1FF)?;
        for &b in bytes {
            // Send data (D/C=1): set bit8
            self.write_command((b & 0xFF) | 0x100)?;
        }
        Ok(())
    }

    #[inline]
    fn edge_delay(&self) {
        if self.half_period_us != 0 {
            {
                Ets::delay_us(self.half_period_us);
            }
        }
    }

    pub fn release(self) -> (SCK, CS, MOSI) {
        (self.sck, self.cs, self.mosi)
    }
}

pub fn init_display<SCK, CS, MOSI>(
    spi: &mut ExpanderSpi<SCK, CS, MOSI>,
) -> Result<(), <ExpanderSpi<SCK, CS, MOSI> as ErrorType>::Error>
where
    SCK: embedded_hal::digital::OutputPin,
    CS: embedded_hal::digital::OutputPin<Error = <SCK as ErrorType>::Error>,
    MOSI: embedded_hal::digital::OutputPin<Error = <SCK as ErrorType>::Error>,
{
    // SWRESET
    //spi.write_command(0xFF)?;
    //FreeRtos::delay_ms(240);

    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13])?;
    spi.write_data(0xEF, &[0x08])?;
    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x10])?;

    spi.write_data(0xC0, &[0x3B, 0x00])?;
    spi.write_data(0xC1, &[0x0B, 0x02])?;
    spi.write_data(0xC2, &[0x00, 0x02])?;
    spi.write_data(0xCC, &[0x10])?;

    // Gamma Positive
    spi.write_data(
        0xB0,
        &[
            0x00, 0x1D, 0x29, 0x12, 0x17, 0x0B, 0x18, 0x09, 0x08, 0x2A, 0x07, 0x14, 0x11, 0x27,
            0x32, 0x1F,
        ],
    )?;

    // Gamma Negative
    spi.write_data(
        0xB1,
        &[
            0x00, 0x1D, 0x29, 0x12, 0x16, 0x0A, 0x18, 0x08, 0x09, 0x2A, 0x07, 0x13, 0x12, 0x27,
            0x33, 0x1F,
        ],
    )?;
    spi.write_data(0xD0, &[0x88]);
    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x11])?;
    spi.write_data(0xB0, &[0x9D])?;
    spi.write_data(0xB1, &[0x24])?;
    spi.write_data(0xB2, &[0x81])?;
    spi.write_data(0xB3, &[0x80])?;
    spi.write_data(0xB5, &[0x43])?;
    spi.write_data(0xB7, &[0x85])?;
    spi.write_data(0xB8, &[0x20])?;
    spi.write_data(0xC1, &[0x78])?;
    spi.write_data(0xC2, &[0x78])?;

    spi.write_data(0xE0, &[0x00, 0x00, 0x02])?;
    spi.write_data(
        0xE1,
        &[
            0x03, 0xA0, 0x00, 0x00, 0x04, 0xA0, 0x00, 0x00, 0x00, 0x20, 0x20,
        ],
    )?;
    spi.write_data(0xE2, &[0x00; 13])?;
    spi.write_data(0xE3, &[0x00, 0x00, 0x11, 0x00])?;
    spi.write_data(0xE4, &[0x22, 0x00])?;
    spi.write_data(
        0xE5,
        &[
            0x05, 0xEC, 0xA0, 0xA0, 0x07, 0xEE, 0xA0, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
    )?;
    spi.write_data(0xE6, &[0x00, 0x00, 0x11, 0x00])?;
    spi.write_data(0xE7, &[0x22, 0x00])?;
    spi.write_data(
        0xE8,
        &[
            0x06, 0xED, 0xA0, 0xA0, 0x08, 0xEF, 0xA0, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
    )?;
    spi.write_data(0xEB, &[0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x00])?;
    spi.write_data(
        0xED,
        &[
            0xFF, 0xFF, 0xFF, 0xBA, 0x0A, 0xBF, 0x45, 0xFF, 0xFF, 0x54, 0xFB, 0xA0, 0xAB, 0xFF,
            0xFF, 0xFF,
        ],
    )?;
    spi.write_data(0xEF, &[0x10, 0x0D, 0x04, 0x08, 0x3F, 0x1F])?;

    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13])?;
    spi.write_data(0xE8, &[0x00, 0x0E])?;
    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x00])?;
    spi.write_data(0x11, &[])?;
    spi.write_data(0xCD, &[0x08])?;
    spi.write_data(0x36, &[0x08])?;
    spi.write_data(0x3A, &[0x66])?;

    FreeRtos::delay_ms(120);

    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00, 0x13])?;
    spi.write_data(0xE8, &[0x00, 0x0C])?;

    FreeRtos::delay_ms(10);

    spi.write_data(0xE8, &[0x00, 0x00])?;
    spi.write_data(0xFF, &[0x77, 0x01, 0x00, 0x00])?;
    spi.write_data(0x29, &[])?;

    FreeRtos::delay_ms(20);

    Ok(())
}

static VSYNC_FLAG: AtomicBool = AtomicBool::new(false);
static BACK_READY: AtomicBool = AtomicBool::new(false);
static BACK_CAN_DRAW: AtomicBool = AtomicBool::new(true);

unsafe extern "C" fn on_vsync_isr(
    _panel: esp_lcd_panel_handle_t,
    _edata: *const esp_lcd_rgb_panel_event_data_t,
    _user_ctx: *mut c_void,
) -> bool {
    VSYNC_FLAG.store(true, Ordering::Release);
    false // no higher-prio task woken
}
// ---------- minimal DrawTarget adapter over back buffer ----------
pub struct EgBackBuffer<'a> {
    buf: &'a mut [u16],
    w: u32,
    h: u32,
}
impl<'a> EgBackBuffer<'a> {
    #[inline]
    pub fn new(buf: &'a mut [u16], w: usize, h: usize) -> Self {
        Self {
            buf,
            w: w as u32,
            h: h as u32,
        }
    }
    #[inline]
    fn set_px(&mut self, x: u32, y: u32, c: Rgb565) {
        if x < self.w && y < self.h {
            self.buf[(y * self.w + x) as usize] = c.into_storage();
        }
    }
    #[inline]
    pub fn clear_fast(&mut self, c: Rgb565) {
        let v = c.into_storage();
        for p in self.buf.iter_mut() {
            *p = v;
        }
    }
}
impl OriginDimensions for EgBackBuffer<'_> {
    fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }
}
impl DrawTarget for EgBackBuffer<'_> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(Point { x, y }, c) in pixels {
            if x >= 0 && y >= 0 {
                self.set_px(x as u32, y as u32, c);
            }
        }
        Ok(())
    }
}

// Framebuffer wrapper with VSYNC
pub struct LcdFbs<'p> {
    panel: esp_lcd_panel_handle_t,
    w: usize,
    h: usize,
    pub front: &'p mut [u16],
    pub back: &'p mut [u16],
    _marker: PhantomData<&'p mut ()>,
}

impl<'p> LcdFbs<'p> {
    /// # Safety
    /// `panel` must be a valid RGB panel configured with double buffering (num_fbs=2).
    pub unsafe fn new(panel: esp_lcd_panel_handle_t, w: usize, h: usize) -> Self {
        // get the two driver framebuffers
        let mut fb0: *mut c_void = core::ptr::null_mut();
        let mut fb1: *mut c_void = core::ptr::null_mut();
        esp!(esp_lcd_rgb_panel_get_frame_buffer(
            panel, 2, &mut fb0, &mut fb1
        ))
        .expect("esp_lcd_rgb_panel_get_frame_buffer");

        let len = w * h;
        let b0 = slice::from_raw_parts_mut(fb0 as *mut u16, len);
        let b1 = slice::from_raw_parts_mut(fb1 as *mut u16, len);

        // register VSYNC callback
        let mut cbs: esp_lcd_rgb_panel_event_callbacks_t = core::mem::zeroed();
        cbs.on_vsync = Some(on_vsync_isr);
        esp!(esp_lcd_rgb_panel_register_event_callbacks(
            panel,
            &cbs,
            core::ptr::null_mut()
        ))
        .expect("register vsync callbacks");

        // clear flag
        VSYNC_FLAG.store(false, Ordering::Release);

        Self {
            panel,
            w,
            h,
            front: b0,
            back: b1,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn back_mut(&mut self) -> &mut [u16] {
        self.back
    }

    #[inline]
    pub fn back_draw_target(&mut self) -> EgBackBuffer<'_> {
        EgBackBuffer::new(self.back, self.w, self.h)
    }

    pub fn show_back(&mut self) {
        let err = unsafe {
            esp_lcd_panel_draw_bitmap(
                self.panel,
                0,
                0,
                self.w as i32,
                self.h as i32,
                self.back.as_ptr() as *const c_void,
            )
        };
        esp!(err).expect("draw_bitmap");
    }

    pub fn refresh(&mut self) {
        let err = unsafe { esp_lcd_rgb_panel_refresh(self.panel) };
        esp!(err).expect("refresh failed");
    }

    pub fn swap_buf(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
    }

    #[inline]
    pub fn clear_fast(&mut self, c: Rgb565) {
        let v16: u16 = c.into_storage();

        if v16 == 0 {
            // super fast clear-to-black: memset bytes to 0
            unsafe {
                core::ptr::write_bytes(
                    self.back.as_mut_ptr() as *mut u8,
                    0u8,
                    self.back.len() * core::mem::size_of::<u16>(),
                );
            }
            return;
        }
    }
}

pub unsafe fn render_task(fbs_ptr: *mut LcdFbs<'static>) {
    let fbs = &mut *fbs_ptr;

    loop {
        if !BACK_CAN_DRAW.load(Ordering::Acquire) {
            //esp_idf_svc::hal::delay::FreeRtos::delay_ms(1);
            continue;
        }
        let mut dt = fbs.back_draw_target(); // always the current back
        dt.clear_fast(Rgb565::BLACK);
        let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        Text::with_alignment(
            "hello world!",
            Point::new(240, 240),
            style,
            Alignment::Center,
        )
        .draw(&mut dt)
        .unwrap();

        Circle::new(Point::new(0, 0), 480)
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLUE, 10))
            .draw(&mut dt)
            .unwrap();

        BACK_READY.store(true, Ordering::Release);
        BACK_CAN_DRAW.store(false, Ordering::Release);
    }
}

pub unsafe fn display_task(fbs_ptr: *mut LcdFbs<'static>) {
    let fbs = &mut *fbs_ptr;
    fbs.show_back();
    let mut bingus: u16 = 0;
    let mut oldpos: u16 = 0;
    loop {
        while !VSYNC_FLAG.swap(false, Ordering::AcqRel) {
            esp_idf_svc::hal::delay::FreeRtos::delay_ms(1);
        }
        bingus = bingus.wrapping_add(1);

        fbs.swap_buf();
        log::info!("Frame");

        let mut dt = fbs.back_draw_target(); // always the current back
                                             //dt.clear_fast(Rgb565::BLACK);
        let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);

        let bb = Text::with_alignment(
            "hello world!",
            Point::new(oldpos.into(), 360),
            style,
            Alignment::Center,
        )
        .bounding_box();
        Rectangle::new(bb.top_left, bb.size)
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(&mut dt)
            .unwrap();

        Text::with_alignment(
            "hello world!",
            Point::new(bingus.into(), 360),
            style,
            Alignment::Center,
        )
        .draw(&mut dt)
        .unwrap();

        //Circle::new(Point::new(0, 0), 720).into_styled(PrimitiveStyle::with_stroke(Rgb565::BLUE, 10)).draw(&mut dt).unwrap();
        //Arc::new(Point::new(0,0), 700, Angle::from_degrees(135.0), Angle::from_degrees(315.0)).into_styled(PrimitiveStyle::with_stroke(Rgb565::CSS_ORANGE, 20)).draw(&mut dt).unwrap();

        oldpos = bingus;
    }
}
