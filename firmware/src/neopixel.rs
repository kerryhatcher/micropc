//! Driver for the onboard WS2812 NeoPixel on the Seeed XIAO RP2040 (GPIO 12).
//!
//! The WS2812 needs ~800 kHz bit timing that the CPU can't reliably bit-bang
//! alongside USB, so a PIO state machine produces the waveform. The FIFO
//! holds one 24-bit GRB word per LED, which the PIO autopulls.

use micropc_shared::Rgb;
use rp2040_hal::{
    pac,
    pio::{Buffers, PIOBuilder, PIOExt, PinDir, Running, SM0, ShiftDirection, StateMachine, Tx},
};

/// GPIO driving the NeoPixel data line.
pub const DATA_PIN: u8 = 12;

/// The PIO runs at 8 MHz: each WS2812 bit takes 10 PIO cycles (1.25 µs).
/// 125 MHz / 8 MHz = 15.625, which is 15 + 160/256 in the divider.
const CLOCK_DIVISOR_INT: u16 = 15;
const CLOCK_DIVISOR_FRAC: u8 = 160;

/// Low time between frames (the WS2812 reset), about 100 µs at 125 MHz.
const RESET_CYCLES: u32 = 12_500;

pub struct NeoPixel {
    tx: Tx<(pac::PIO0, SM0)>,
    // Kept alive so the state machine keeps running.
    _sm: StateMachine<(pac::PIO0, SM0), Running>,
}

impl NeoPixel {
    /// Claims PIO0 state machine 0 and starts it on [`DATA_PIN`].
    ///
    /// The caller must have switched `DATA_PIN` into PIO function and must
    /// have powered the NeoPixel (GPIO 11 high).
    pub fn new(pio0: pac::PIO0, resets: &mut pac::RESETS) -> Self {
        let (mut pio_block, sm0, _, _, _) = pio0.split(resets);

        // Standard WS2812 program (800 kHz, 1-bit side-set on the data pin).
        let program = pio::pio_asm!(
            ".side_set 1",
            ".define public T1 2",
            ".define public T2 5",
            ".define public T3 3",
            ".wrap_target",
            "bitloop:",
            "    out x, 1       side 0 [T3 - 1]",
            "    jmp !x do_zero side 1 [T1 - 1]",
            "    jmp bitloop    side 1 [T2 - 1]",
            "do_zero:",
            "    nop            side 0 [T2 - 1]",
            ".wrap",
        );
        let installed = pio_block
            .install(&program.program)
            .expect("WS2812 program fits in PIO0");

        let (mut sm, _rx, tx) = PIOBuilder::from_installed_program(installed)
            .side_set_pin_base(DATA_PIN)
            .clock_divisor_fixed_point(CLOCK_DIVISOR_INT, CLOCK_DIVISOR_FRAC)
            .out_shift_direction(ShiftDirection::Left)
            .autopull(true)
            .pull_threshold(24)
            .buffers(Buffers::OnlyTx)
            .build(sm0);

        sm.set_pindirs([(DATA_PIN, PinDir::Output)]);
        let running = sm.start();

        Self { tx, _sm: running }
    }

    /// Show `color` on the NeoPixel.
    pub fn show(&mut self, color: Rgb) {
        // WS2812 takes GRB order, most significant bit first. The PIO pulls
        // 24 bits from the top of the 32-bit word, hence the shift.
        let grb = (u32::from(color.g) << 16) | (u32::from(color.r) << 8) | u32::from(color.b);
        while !self.tx.write(grb << 8) {}

        // Wait for the word to leave the FIFO, then hold the line low for the reset.
        while !self.tx.is_empty() {}
        cortex_m::asm::delay(RESET_CYCLES);
    }
}
