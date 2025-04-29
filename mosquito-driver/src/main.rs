#![no_std]
#![no_main]
#![cfg(not(test))]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::timer::input_capture::{CapturePin, Ch3, InputCapture};
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::{
    spi::{BitOrder, Config as SpiConfig, MODE_1, Spi},
    time::Hertz,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

embassy_stm32::bind_interrupts!(struct Irqs {
    TIM2 => embassy_stm32::timer::CaptureCompareInterruptHandler<embassy_stm32::peripherals::TIM2>;
});

type AnglePwmInput = InputCapture<'static, embassy_stm32::peripherals::TIM2>;
struct AnglePwmData {
    pub high: u32,
    pub low: u32,
    pub period: u32,
}
impl Default for AnglePwmData {
    fn default() -> Self {
        AnglePwmData {
            high: 0,
            low: 1,
            period: 1,
        }
    }
}

static ANGLE_PWM_DATA: Signal<CriticalSectionRawMutex, AnglePwmData> = Signal::new();

#[embassy_executor::task]
async fn angle_pwm_reader(pin: AnglePwmInput) {
    let mut pin = pin;
    let mut last_rising = 0;
    loop {
        let falling = pin
            .wait_for_falling_edge(embassy_stm32::timer::Channel::Ch3)
            .await;
        let high = falling - last_rising;

        let rising = pin
            .wait_for_rising_edge(embassy_stm32::timer::Channel::Ch3)
            .await;
        let low = rising - falling;
        let period = rising - last_rising;
        last_rising = rising;

        ANGLE_PWM_DATA.signal(AnglePwmData { high, low, period });
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting up!");
    let p = embassy_stm32::init(Default::default());

    let mut spi_config = SpiConfig::default();
    spi_config.mode = MODE_1;
    spi_config.bit_order = BitOrder::MsbFirst;
    spi_config.frequency = Hertz(4_000_000);
    let mut spi = Spi::new_rxonly(p.SPI1, p.PA5, p.PA6, p.DMA1_CH2, p.DMA1_CH1, spi_config);
    // let mut spi = Spi::new(
    //     p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH4, spi_config,
    // );
    let mut cs = Output::new(p.PA4, Level::High, Speed::Low);
    cs.set_high();

    let angle_pwm_pin: CapturePin<embassy_stm32::peripherals::TIM2, Ch3> =
        CapturePin::new_ch3(p.PC6, Pull::None);
    let angle_pwm_input = InputCapture::new(
        p.TIM2,
        None,
        None,
        Some(angle_pwm_pin),
        None,
        Irqs,
        Hertz(1_000_000),
        //Hertz(10_000),
        CountingMode::EdgeAlignedUp,
    );
    spawner.spawn(angle_pwm_reader(angle_pwm_input)).unwrap();

    loop {
        //let send_buffer = [0xFFFFu16; 1];
        //let send_buffer = [0x0u16; 1];
        let mut recv_buffer = [0u16; 1];
        cs.set_low();
        Timer::after_millis(200).await;
        spi.read(&mut recv_buffer).await.unwrap();
        // spi.transfer(&mut recv_buffer, &send_buffer[..])
        //     .await
        //     .unwrap();

        if let Some(data) = ANGLE_PWM_DATA.try_take() {
            info!(
                "angle {} H {} L {} P {}",
                recv_buffer[0], data.high, data.low, data.period
            );
        } else {
            info!("angle {} no PWM data", recv_buffer[0]);
        }

        Timer::after_millis(300).await;
        cs.set_high();

        Timer::after_millis(500).await;
    }
}
