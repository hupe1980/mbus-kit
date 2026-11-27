//! Every code of the `FD`, `FB` and combinable VIFE tables, checked against the tables
//! of EN 13757-3:2018 as listed in OMS Vol. 2 Annex B and the m-bus.com documentation
//! (spec 001, SC-001). Each range is checked at both ends; reserved codes are checked
//! as an exact set, so a code that silently moves into or out of "reserved" fails.

use mbus_kit::vif::{Event, Extension, Particles, Per, Table, Times};
use mbus_kit::{Kind, Modifier, Records, Unit, Value};

/// The first record of `bytes`.
fn rec(bytes: &[u8]) -> mbus_kit::Record<'_> {
    Records::new(bytes).next().expect("a record").expect("a valid record")
}

/// Description of `VIF code` with a 1-byte integer `00`.
fn desc(vif: u8, code: u8) -> (Kind, Option<Unit>, i8) {
    let bytes = [0x01, vif, code, 0x00];
    let d = rec(&bytes).description();
    (d.kind, d.unit, d.exponent)
}

#[test]
fn fd_table() {
    let fd = |c| desc(0xFD, c);
    let cases: &[(u8, Kind, Option<Unit>, i8)] = &[
        (0x00, Kind::Credit, Some(Unit::Currency), -3),
        (0x03, Kind::Credit, Some(Unit::Currency), 0),
        (0x04, Kind::Debit, Some(Unit::Currency), -3),
        (0x07, Kind::Debit, Some(Unit::Currency), 0),
        (0x08, Kind::AccessNumber, None, 0),
        (0x09, Kind::Medium, None, 0),
        (0x0A, Kind::Manufacturer, None, 0),
        (0x0B, Kind::ParameterSetId, None, 0),
        (0x0C, Kind::ModelVersion, None, 0),
        (0x0D, Kind::HardwareVersion, None, 0),
        (0x0E, Kind::FirmwareVersion, None, 0),
        (0x0F, Kind::SoftwareVersion, None, 0),
        (0x10, Kind::CustomerLocation, None, 0),
        (0x11, Kind::Customer, None, 0),
        (0x12, Kind::AccessCodeUser, None, 0),
        (0x13, Kind::AccessCodeOperator, None, 0),
        (0x14, Kind::AccessCodeSystemOperator, None, 0),
        (0x15, Kind::AccessCodeDeveloper, None, 0),
        (0x16, Kind::Password, None, 0),
        (0x17, Kind::ErrorFlags, None, 0),
        (0x18, Kind::ErrorMask, None, 0),
        (0x19, Kind::SecurityKey, None, 0),
        (0x1A, Kind::DigitalOutput, None, 0),
        (0x1B, Kind::DigitalInput, None, 0),
        (0x1C, Kind::BaudRate, Some(Unit::Baud), 0),
        (0x1D, Kind::ResponseDelay, Some(Unit::BitTimes), 0),
        (0x1E, Kind::Retry, None, 0),
        (0x1F, Kind::RemoteControl, None, 0),
        (0x20, Kind::FirstStorageNumber, None, 0),
        (0x21, Kind::LastStorageNumber, None, 0),
        (0x22, Kind::StorageBlockSize, None, 0),
        (0x23, Kind::TariffDescriptor, None, 0),
        (0x24, Kind::StorageInterval, Some(Unit::Seconds), 0),
        (0x27, Kind::StorageInterval, Some(Unit::Days), 0),
        (0x28, Kind::StorageInterval, Some(Unit::Months), 0),
        (0x29, Kind::StorageInterval, Some(Unit::Years), 0),
        (0x2A, Kind::OperatorSpecific, None, 0),
        (0x2B, Kind::TimePointSecond, Some(Unit::Seconds), 0),
        (0x2C, Kind::DurationSinceReadout, Some(Unit::Seconds), 0),
        (0x2F, Kind::DurationSinceReadout, Some(Unit::Days), 0),
        (0x30, Kind::TariffStart, None, 0),
        (0x31, Kind::TariffDuration, Some(Unit::Minutes), 0),
        (0x33, Kind::TariffDuration, Some(Unit::Days), 0),
        (0x34, Kind::TariffPeriod, Some(Unit::Seconds), 0),
        (0x37, Kind::TariffPeriod, Some(Unit::Days), 0),
        (0x38, Kind::TariffPeriod, Some(Unit::Months), 0),
        (0x39, Kind::TariffPeriod, Some(Unit::Years), 0),
        (0x3A, Kind::Dimensionless, None, 0),
        (0x3B, Kind::WmbusContainer, None, 0),
        (0x3C, Kind::TransmissionPeriod, Some(Unit::Seconds), 0),
        (0x3F, Kind::TransmissionPeriod, Some(Unit::Days), 0),
        (0x40, Kind::Voltage, Some(Unit::Volt), -9),
        (0x4F, Kind::Voltage, Some(Unit::Volt), 6),
        (0x50, Kind::Current, Some(Unit::Ampere), -12),
        (0x5F, Kind::Current, Some(Unit::Ampere), 3),
        (0x60, Kind::ResetCounter, None, 0),
        (0x61, Kind::CumulationCounter, None, 0),
        (0x62, Kind::ControlSignal, None, 0),
        (0x63, Kind::DayOfWeek, None, 0),
        (0x64, Kind::WeekNumber, None, 0),
        (0x65, Kind::DayChange, None, 0),
        (0x66, Kind::ParameterActivation, None, 0),
        (0x67, Kind::SupplierInformation, None, 0),
        (0x68, Kind::DurationSinceCumulation, Some(Unit::Hours), 0),
        (0x6B, Kind::DurationSinceCumulation, Some(Unit::Years), 0),
        (0x6C, Kind::BatteryOperatingTime, Some(Unit::Hours), 0),
        (0x6F, Kind::BatteryOperatingTime, Some(Unit::Years), 0),
        (0x70, Kind::BatteryChange, None, 0),
        (0x71, Kind::SignalLevel, Some(Unit::Dbm), 0),
        (0x72, Kind::DaylightSaving, None, 0),
        (0x73, Kind::ListeningWindow, None, 0),
        (0x74, Kind::RemainingBatteryLife, Some(Unit::Days), 0),
        (0x75, Kind::StopCount, None, 0),
        (0x76, Kind::ManufacturerContainer, None, 0),
    ];
    for &(code, kind, unit, exp) in cases {
        assert_eq!(fd(code), (kind, unit, exp), "FD {code:02x}");
    }
    // Exactly these codes are reserved; 7D selects the second-level table.
    for code in 0..0x80u8 {
        let reserved = matches!(fd(code).0, Kind::Reserved { table: Table::Fd, .. });
        let expected = matches!(code, 0x77..=0x7C | 0x7E | 0x7F);
        assert_eq!(reserved, expected, "FD {code:02x}");
    }
}

#[test]
fn fd_second_level_table() {
    let d = |c| rec(&[0x01, 0xFD, 0xFD, c, 0x00]).description();
    assert_eq!(d(0x02).unit, Some(Unit::Months));
    assert_eq!((d(0x11).kind, d(0x11).unit, d(0x11).exponent), (Kind::CarbonDioxide, Some(Unit::Ppm), 1));
    assert_eq!(d(0x19).kind, Kind::Particles(Particles::Pm2_5));
    assert_eq!(d(0x2E).kind, Kind::Events(Event::Alarms));
    assert_eq!((d(0x27).unit, d(0x27).exponent), (Some(Unit::Ph), -1));
    assert_eq!(d(0x45).kind, Kind::Reserved { table: Table::FdSecond, code: 0x45 });
}

#[test]
fn fb_table() {
    let fb = |c| desc(0xFB, c);
    let cases: &[(u8, Kind, Option<Unit>, i8)] = &[
        (0x00, Kind::Energy, Some(Unit::Wh), 5), // 10^-1 MWh
        (0x01, Kind::Energy, Some(Unit::Wh), 6),
        (0x02, Kind::ReactiveEnergy, Some(Unit::VarH), 3), // kvarh
        (0x03, Kind::ReactiveEnergy, Some(Unit::VarH), 4),
        (0x04, Kind::ApparentEnergy, Some(Unit::VaH), 3),
        (0x08, Kind::Energy, Some(Unit::J), 8), // 10^-1 GJ
        (0x09, Kind::Energy, Some(Unit::J), 9),
        (0x0C, Kind::Energy, Some(Unit::Calorie), 5), // 10^-1 Mcal
        (0x0F, Kind::Energy, Some(Unit::Calorie), 8),
        (0x10, Kind::Volume, Some(Unit::M3), 2),
        (0x11, Kind::Volume, Some(Unit::M3), 3),
        (0x14, Kind::ReactivePower, Some(Unit::Var), 0), // 10^-3 kvar
        (0x17, Kind::ReactivePower, Some(Unit::Var), 3),
        (0x18, Kind::Mass, Some(Unit::Kg), 5), // 10^2 t
        (0x19, Kind::Mass, Some(Unit::Kg), 6),
        (0x1A, Kind::RelativeHumidity, Some(Unit::Percent), -1),
        (0x1B, Kind::RelativeHumidity, Some(Unit::Percent), 0),
        (0x20, Kind::Volume, Some(Unit::CubicFeet), 0),
        (0x21, Kind::Volume, Some(Unit::CubicFeet), -1),
        (0x22, Kind::Volume, Some(Unit::UsGallon), -1),
        (0x23, Kind::Volume, Some(Unit::UsGallon), 0),
        (0x24, Kind::VolumeFlow, Some(Unit::UsGallonPerMin), -3),
        (0x25, Kind::VolumeFlow, Some(Unit::UsGallonPerMin), 0),
        (0x26, Kind::VolumeFlow, Some(Unit::UsGallonPerH), 0),
        (0x28, Kind::Power, Some(Unit::W), 5), // 10^-1 MW
        (0x29, Kind::Power, Some(Unit::W), 6),
        (0x2A, Kind::PhaseVoltageVoltage, Some(Unit::Degree), -1),
        (0x2B, Kind::PhaseVoltageCurrent, Some(Unit::Degree), -1),
        (0x2C, Kind::Frequency, Some(Unit::Hertz), -3),
        (0x2F, Kind::Frequency, Some(Unit::Hertz), 0),
        (0x30, Kind::Power, Some(Unit::JPerH), 8), // 10^-1 GJ/h
        (0x31, Kind::Power, Some(Unit::JPerH), 9),
        (0x34, Kind::ApparentPower, Some(Unit::VoltAmpere), 0),
        (0x58, Kind::FlowTemperature, Some(Unit::Fahrenheit), -3),
        (0x5F, Kind::ReturnTemperature, Some(Unit::Fahrenheit), 0),
        (0x60, Kind::TemperatureDifference, Some(Unit::Fahrenheit), -3),
        (0x67, Kind::ExternalTemperature, Some(Unit::Fahrenheit), 0),
        (0x70, Kind::TemperatureLimit, Some(Unit::Fahrenheit), -3),
        (0x77, Kind::TemperatureLimit, Some(Unit::Celsius), 0),
        (0x78, Kind::CumulativeMaxPower, Some(Unit::W), -3),
        (0x7F, Kind::CumulativeMaxPower, Some(Unit::W), 4),
    ];
    for &(code, kind, unit, exp) in cases {
        assert_eq!(fb(code), (kind, unit, exp), "FB {code:02x}");
    }
    for code in 0..0x80u8 {
        let reserved = matches!(fb(code).0, Kind::Reserved { table: Table::Fb, .. });
        let expected = matches!(code, 0x06 | 0x07 | 0x0A | 0x0B | 0x12 | 0x13 | 0x1C..=0x1F | 0x27 | 0x32 | 0x33 | 0x38..=0x57 | 0x68..=0x6F);
        assert_eq!(reserved, expected, "FB {code:02x}");
    }
}

/// The modifier of combinable VIFE `code` after a volume VIF.
fn modifier(code: u8) -> Modifier {
    let bytes = [0x01, 0x93, code & 0x7F, 0x00];
    rec(&bytes).vib.modifiers().next().expect("a modifier")
}

#[test]
fn combinable_table() {
    let cases: &[(u8, Modifier)] = &[
        (0x15, Modifier::Error(mbus_kit::vif::RecordError(0x15))),
        (0x12, Modifier::Average),
        (0x13, Modifier::CompactProfile(mbus_kit::vif::Direction::Inverse)),
        (0x14, Modifier::RelativeDeviation),
        (0x1D, Modifier::StandardConformant),
        (0x1F, Modifier::CompactProfile(mbus_kit::vif::Direction::Forward)),
        (0x20, Modifier::Per(Per::Second)),
        (0x26, Modifier::Per(Per::Year)),
        (0x27, Modifier::Per(Per::Revolution)),
        (0x28, Modifier::IncrementPerPulse { output: false, channel: 0 }),
        (0x2B, Modifier::IncrementPerPulse { output: true, channel: 1 }),
        (0x2C, Modifier::Per(Per::Litre)),
        (0x35, Modifier::Per(Per::Ampere)),
        (0x36, Modifier::MultipliedBy(Times::Second)),
        (0x38, Modifier::MultipliedBy(Times::SecondPerAmpere)),
        (0x39, Modifier::StartDateOf),
        (0x3A, Modifier::UncorrectedUnit),
        (0x3B, Modifier::Forward),
        (0x3C, Modifier::Backward),
        (0x3D, Modifier::NonMetric),
        (0x3E, Modifier::BaseConditions),
        (0x3F, Modifier::ObisDeclaration),
        (0x40, Modifier::Limit { upper: false }),
        (0x48, Modifier::Limit { upper: true }),
        (0x41, Modifier::ExceedCount { upper: false }),
        (0x49, Modifier::ExceedCount { upper: true }),
        (0x42, Modifier::LimitExceedDate { upper: false, last: false, end: false }),
        (0x47, Modifier::LimitExceedDate { upper: false, last: true, end: true }),
        (0x4E, Modifier::LimitExceedDate { upper: true, last: true, end: false }),
        (0x4F, Modifier::LimitExceedDate { upper: true, last: true, end: true }),
        (0x44, Modifier::Reserved(0x44)),
        (0x4D, Modifier::Reserved(0x4D)),
        (0x50, Modifier::LimitExceedDuration { upper: false, last: false, unit: Unit::Seconds }),
        (0x5F, Modifier::LimitExceedDuration { upper: true, last: true, unit: Unit::Days }),
        (0x60, Modifier::Duration { last: false, unit: Unit::Seconds }),
        (0x67, Modifier::Duration { last: true, unit: Unit::Days }),
        (0x68, Modifier::ValueDuringLimitExceed { upper: false }),
        (0x6C, Modifier::ValueDuringLimitExceed { upper: true }),
        (0x69, Modifier::Leakage),
        (0x6D, Modifier::Overflow),
        (0x6A, Modifier::DateOf { last: false, end: false }),
        (0x6F, Modifier::DateOf { last: true, end: true }),
        (0x70, Modifier::Scale(-6)),
        (0x77, Modifier::Scale(1)),
        (0x78, Modifier::Offset(-3)),
        (0x7B, Modifier::Offset(0)),
        (0x7D, Modifier::Scale(3)),
        (0x7E, Modifier::FutureValue),
        (0x7F, Modifier::ManufacturerSpecific),
    ];
    for &(code, m) in cases {
        assert_eq!(modifier(code | 0x80), m, "VIFE {code:02x}");
    }
    // The extension table after combinable FC.
    let ext = |c| rec(&[0x01, 0x93, 0xFC, c, 0x00]).vib.modifiers().next().unwrap();
    assert_eq!(ext(0x02), Modifier::Extension(Extension::Phase(2)));
    assert_eq!(ext(0x07), Modifier::Extension(Extension::BetweenPhases(3)));
    assert_eq!(ext(0x0C), Modifier::Extension(Extension::ImportExportDelta));
    assert_eq!(ext(0x11), Modifier::Extension(Extension::Unsigned));
    assert_eq!(ext(0x7A), Modifier::Extension(Extension::Other(0x7A)));
}

// ---- 001-describe-every-record/SR-001: corrections are exact, unknowns give no quantity

#[test]
fn correction_multiplicative_is_applied_exactly() {
    // VIF 13 (10^-3 m³), VIFE 75 (10^-1), value 12345 → 1.2345 m³.
    let r = rec(&[0x02, 0x93, 0x75, 0x39, 0x30]);
    let q = r.quantity().unwrap();
    assert_eq!((q.mantissa, q.exponent), (12345, -4));
    assert_eq!(q.to_string(), "1.2345 m³");
    // 7D: × 10^3.
    let r = rec(&[0x01, 0x93, 0x7D, 0x05]);
    assert_eq!(r.quantity().unwrap().exponent, 0);
    // Two corrections add.
    let r = rec(&[0x01, 0x93, 0xFD, 0x75, 0x05]);
    assert_eq!(r.quantity().unwrap().exponent, -1);
}

#[test]
fn correction_chains_stay_in_range() {
    // The 10-VIFE cap bounds the exponent: FB 7F (10^4 W) and nine × 10^3 is 10^31,
    // ten × 10^-6 after 10^-12 A is 10^-72; both fit i8, and the arithmetic is checked
    // anyway. The extreme chains give exact quantities, never a wrapped exponent.
    let r = rec(&[0x01, 0xFB, 0xFF, 0xFD, 0xFD, 0xFD, 0xFD, 0xFD, 0xFD, 0xFD, 0xFD, 0x7D, 0x01]);
    assert_eq!(r.quantity().unwrap().exponent, 4 + 27);
    let r = rec(&[0x01, 0xFD, 0xD0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0x70, 0x01]);
    assert_eq!(r.quantity().unwrap().exponent, -12 - 54);
}

#[test]
fn modifiers_that_change_meaning_give_no_quantity() {
    for code in [0x22u8, 0x28, 0x36, 0x39, 0x41, 0x42, 0x50, 0x60, 0x78, 0x15, 0x7F, 0x44, 0x3D] {
        let bytes = [0x01, 0x93, code, 0x05];
        assert_eq!(rec(&bytes).quantity(), None, "VIFE {code:02x}");
    }
    for code in [0x3A, 0x3B, 0x3C, 0x40, 0x48, 0x68, 0x69, 0x6D, 0x7E, 0x12] {
        let bytes = [0x01, 0x93, code, 0x05];
        assert!(rec(&bytes).quantity().is_some(), "VIFE {code:02x}");
    }
}

#[test]
fn date_modifiers_decode_dates() {
    // VIFE 39 (start date of) on a 2-byte value: type G.
    let r = rec(&[0x02, 0x93, 0x39, 0x5B, 0x3B]);
    assert_eq!(r.value.to_string(), "2026-11-27");
    // FD 70: battery change, 4 bytes: type F.
    let r = rec(&[0x04, 0xFD, 0x70, 0x1E, 0x0E, 0x5B, 0x3B]);
    assert_eq!(r.value.to_string(), "2026-11-27 14:30");
}

#[test]
fn unsigned_extension_reads_data_type_c() {
    let r = rec(&[0x02, 0x6E | 0x80, 0xFC, 0x11, 0xFF, 0xFF]);
    assert_eq!(r.value, Value::UInt(65535));
    let r = rec(&[0x02, 0x6E, 0xFF, 0xFF]);
    assert_eq!(r.value, Value::Int(-1));
}

#[test]
fn data_types_i_and_j() {
    // VIF 6D with 6 bytes: type I with seconds.
    let r = rec(&[0x06, 0x6D, 0x3B, 0x1E, 0x0E, 0x5B, 0x3B, 0x30]);
    let Value::DateTime(t) = r.value else { panic!("{:?}", r.value) };
    assert!(t.has_seconds);
    assert_eq!(t.time.second, 59);
    // VIF 6D with 3 bytes: type J.
    let r = rec(&[0x03, 0x6D, 0x05, 0x04, 0x03]);
    assert_eq!(r.value.to_string(), "03:04:05");
}

#[test]
fn lvar_f0_to_f6_are_long_binary_numbers() {
    let mut bytes = [0u8; 3 + 16];
    bytes[0] = 0x0D;
    bytes[1] = 0x13;
    bytes[2] = 0xF0;
    let r = rec(&bytes);
    assert!(matches!(r.value, Value::Bytes(b) if b.len() == 16));
    // F7–FF are reserved.
    assert!(Records::new(&[0x0D, 0x13, 0xF7]).next().unwrap().is_err());
    let mut long = [0u8; 3 + 64];
    long[..3].copy_from_slice(&[0x0D, 0x13, 0xF6]);
    assert!(matches!(rec(&long).value, Value::Bytes(b) if b.len() == 64));
}

// ---- 001-describe-every-record/SR-003: meter text never reaches a terminal raw

#[test]
fn display_escapes_meter_text() {
    // LVAR text "ESC [2J" (sent reversed).
    let r = rec(&[0x0D, 0xFD, 0x11, 0x04, b'J', b'2', b'[', 0x1B]);
    let s = r.to_string();
    assert!(s.contains("\\x1b[2J"), "{s}");
    assert!(!s.bytes().any(|b| b.is_ascii_control()), "{s:?}");
    // Plain-text unit with a control byte.
    let r = rec(&[0x01, 0x7C, 0x02, 0x07, b'a', 0x2A]);
    assert!(!r.to_string().bytes().any(|b| b.is_ascii_control()));
}

#[test]
fn display_reads_like_a_sentence() {
    let r = rec(&[0x01, 0xFD, 0x08, 0x55]);
    assert_eq!(r.to_string(), "access number — 85");
    let r = rec(&[0x02, 0x93, 0x75, 0x39, 0x30]);
    assert_eq!(r.to_string(), "volume — 1.2345 m³");
    let r = rec(&[0x01, 0x93, 0x22, 0x05]);
    assert_eq!(r.to_string(), "volume · per hour — 5");
}
