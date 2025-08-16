#![no_std]

pub mod display;
use core::time::Duration;

use embedded_hal::digital::{ErrorType, OutputPin};
use esp_idf_hal::{
    delay::{Ets, FreeRtos},
    sys,
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
    spi.write_data(0xD0, &[0x88])?;
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

pub fn now() -> Duration {
    let us = unsafe { sys::esp_timer_get_time() } as u64;
    Duration::from_micros(us)
}
