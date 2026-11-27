//! The value information block (EN 13757-3:2018, clause 6.4): what a value is.
//!
//! ```text
//! VIF  [VIFE …]
//! ```
//!
//! The VIF selects a quantity, its unit and decimal exponent from the **primary table**.
//! Two VIF codes point to an extension table, whose code is the first VIFE: `FB`
//! (larger units, °F, volume in US units) and `FD` (identifiers, versions, error flags,
//! volts, amperes, storage and tariff metadata). Every other VIFE is **combinable**: it
//! modifies the value ("per hour", "start date of", "upper limit", "× 10⁻²", "error:
//! no data available").
//!
//! [`Vib::description`] answers *what* the value is; [`Vib::modifiers`] lists the
//! combinable VIFEs. [`Record::quantity`](crate::Record::quantity) applies the
//! multiplicative corrections and refuses to return a quantity when a modifier changes
//! unit or meaning in a way this crate does not model.

use core::fmt;

use crate::value::Unit;

/// The most VIFE (and DIFE) bytes that may follow one another.
pub(crate) const MAX_EXTENSIONS: usize = 10;

/// The value information block: VIF, up to 10 VIFEs, and a plain-text unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vib<'a> {
    vif: u8,
    vifes: [u8; MAX_EXTENSIONS],
    vife_len: u8,
    plain_text: Option<&'a [u8]>,
}

/// Which table a code came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Table {
    /// The primary VIF table (EN 13757-3, Table 10).
    Primary,
    /// The extension table after VIF `FB` (Table 14).
    Fb,
    /// The extension table after VIF `FD` (Table 12).
    Fd,
    /// The second-level table after `FD FD` (OMS sensors; EN 13757-3:2025).
    FdSecond,
    /// The reserved third extension table after VIF `EF`.
    Third,
}

/// What a value is, independent of which table said so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    // ---- primary table
    /// Energy (Wh, J, and from `FB` megawatt-hours, gigajoules, megacalories).
    Energy,
    /// Volume (m³, ft³, US gallons).
    Volume,
    /// Mass (kg, t).
    Mass,
    /// Time the meter has been switched on.
    OnTime,
    /// Time the meter has been measuring.
    OperatingTime,
    /// Power (W, J/h, MW, GJ/h).
    Power,
    /// Volume flow.
    VolumeFlow,
    /// Mass flow.
    MassFlow,
    /// Flow (supply) temperature.
    FlowTemperature,
    /// Return temperature.
    ReturnTemperature,
    /// Temperature difference.
    TemperatureDifference,
    /// External (outside) temperature.
    ExternalTemperature,
    /// Pressure.
    Pressure,
    /// A date (type G).
    Date,
    /// A date and time (type F or I), or a time of day (type J).
    DateTime,
    /// Units for heat cost allocator (dimensionless).
    HcaUnits,
    /// Averaging duration.
    AveragingDuration,
    /// Actuality duration: time since the value was measured.
    ActualityDuration,
    /// Fabrication number.
    FabricationNumber,
    /// (Enhanced) identification.
    Identification,
    /// Bus address.
    BusAddress,
    /// The unit is given as text (VIF `7C`/`FC`), see [`Vib::plain_text`].
    PlainText,
    /// Any VIF (`7E`/`FE`): readout selection by a master.
    AnyVif,
    /// Manufacturer-specific (`7F`/`FF`).
    ManufacturerSpecific,

    // ---- FD table
    /// Credit, in the local legal currency.
    Credit,
    /// Debit, in the local legal currency.
    Debit,
    /// Unique message identification (access number).
    AccessNumber,
    /// Device type, as in the header.
    Medium,
    /// Manufacturer, as in the header.
    Manufacturer,
    /// Parameter set identification.
    ParameterSetId,
    /// Model / version.
    ModelVersion,
    /// Hardware version.
    HardwareVersion,
    /// Metrology (firmware) version.
    FirmwareVersion,
    /// Other software version.
    SoftwareVersion,
    /// Customer location.
    CustomerLocation,
    /// Customer.
    Customer,
    /// Access code, user.
    AccessCodeUser,
    /// Access code, operator.
    AccessCodeOperator,
    /// Access code, system operator.
    AccessCodeSystemOperator,
    /// Access code, developer.
    AccessCodeDeveloper,
    /// Password.
    Password,
    /// Error flags (binary, device-specific bits).
    ErrorFlags,
    /// Error mask.
    ErrorMask,
    /// Security key.
    SecurityKey,
    /// Digital output (binary).
    DigitalOutput,
    /// Digital input (binary).
    DigitalInput,
    /// Baud rate.
    BaudRate,
    /// Response delay time, in bit times.
    ResponseDelay,
    /// Retry.
    Retry,
    /// Remote control (device-specific).
    RemoteControl,
    /// First storage number for cyclic storage.
    FirstStorageNumber,
    /// Last storage number for cyclic storage.
    LastStorageNumber,
    /// Size of a storage block.
    StorageBlockSize,
    /// Descriptor for tariff and subunit.
    TariffDescriptor,
    /// Storage interval.
    StorageInterval,
    /// Operator-specific data.
    OperatorSpecific,
    /// Time point, second (0–59).
    TimePointSecond,
    /// Duration since the last readout.
    DurationSinceReadout,
    /// Start (date and time) of a tariff.
    TariffStart,
    /// Duration of a tariff.
    TariffDuration,
    /// Period of a tariff.
    TariffPeriod,
    /// Dimensionless: no VIF.
    Dimensionless,
    /// Data container for the wireless M-Bus protocol.
    WmbusContainer,
    /// Period of nominal data transmissions.
    TransmissionPeriod,
    /// Voltage.
    Voltage,
    /// Current.
    Current,
    /// Reset counter.
    ResetCounter,
    /// Cumulation counter.
    CumulationCounter,
    /// Control signal.
    ControlSignal,
    /// Day of week.
    DayOfWeek,
    /// Week number.
    WeekNumber,
    /// Time point of day change.
    DayChange,
    /// State of parameter activation.
    ParameterActivation,
    /// Special supplier information.
    SupplierInformation,
    /// Duration since the last cumulation.
    DurationSinceCumulation,
    /// Operating time of the battery.
    BatteryOperatingTime,
    /// Date and time of the battery change.
    BatteryChange,
    /// Received signal level (RSSI).
    SignalLevel,
    /// Daylight saving: beginning, ending, deviation (data type K).
    DaylightSaving,
    /// Listening window management (data type L).
    ListeningWindow,
    /// Remaining battery lifetime.
    RemainingBatteryLife,
    /// Number of times the meter was stopped.
    StopCount,
    /// Data container for a manufacturer-specific protocol.
    ManufacturerContainer,

    // ---- FB table
    /// Reactive energy.
    ReactiveEnergy,
    /// Apparent energy.
    ApparentEnergy,
    /// Reactive power.
    ReactivePower,
    /// Apparent power.
    ApparentPower,
    /// Relative humidity.
    RelativeHumidity,
    /// Phase angle between two voltages.
    PhaseVoltageVoltage,
    /// Phase angle between voltage and current.
    PhaseVoltageCurrent,
    /// Frequency.
    Frequency,
    /// Cold / warm temperature limit.
    TemperatureLimit,
    /// Cumulative count of maximum power.
    CumulativeMaxPower,

    // ---- FD FD second-level table (OMS Vol. 2 Annex B)
    /// Currently selected application.
    SelectedApplication,
    /// List of sub-device types.
    SubDeviceTypes,
    /// Carbon dioxide content.
    CarbonDioxide,
    /// Carbon monoxide content.
    CarbonMonoxide,
    /// Volatile organic compounds.
    VolatileOrganicCompounds,
    /// Particles (unspecified range, PM1, PM2.5 or PM10, see [`Kind::name`]).
    Particles(Particles),
    /// Illuminance.
    Illuminance,
    /// Luminous intensity.
    LuminousIntensity,
    /// Irradiance (radiant flux density).
    Irradiance,
    /// Wind speed.
    WindSpeed,
    /// Rainfall.
    Rainfall,
    /// Sound level, A-weighted.
    SoundLevel,
    /// Turbidity.
    Turbidity,
    /// pH.
    Ph,
    /// A counter of events of a smoke or sensor device.
    Events(Event),
    /// Battery status.
    BatteryStatus,
    /// Smoke chamber pollution level.
    ChamberPollution,
    /// Distance.
    Distance,
    /// Moisture level.
    Moisture,
    /// Status bits of a sensor.
    StatusBits(Sensor),

    /// A code the table reserves.
    Reserved {
        /// The table.
        table: Table,
        /// The code, extension bit cleared.
        code: u8,
    },
    /// VIF `FB`/`FD` whose table code is missing (the extension bit was clear).
    MissingExtension(Table),
}

impl Kind {
    /// A short English name, e.g. `"volume flow"`.
    #[allow(clippy::too_many_lines)]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Energy => "energy",
            Self::Volume => "volume",
            Self::Mass => "mass",
            Self::OnTime => "on time",
            Self::OperatingTime => "operating time",
            Self::Power => "power",
            Self::VolumeFlow => "volume flow",
            Self::MassFlow => "mass flow",
            Self::FlowTemperature => "flow temperature",
            Self::ReturnTemperature => "return temperature",
            Self::TemperatureDifference => "temperature difference",
            Self::ExternalTemperature => "external temperature",
            Self::Pressure => "pressure",
            Self::Date => "date",
            Self::DateTime => "date and time",
            Self::HcaUnits => "heat cost allocation",
            Self::AveragingDuration => "averaging duration",
            Self::ActualityDuration => "actuality duration",
            Self::FabricationNumber => "fabrication number",
            Self::Identification => "identification",
            Self::BusAddress => "bus address",
            Self::PlainText => "plain-text unit",
            Self::AnyVif => "any VIF",
            Self::ManufacturerSpecific => "manufacturer specific",
            Self::Credit => "credit",
            Self::Debit => "debit",
            Self::AccessNumber => "access number",
            Self::Medium => "medium",
            Self::Manufacturer => "manufacturer",
            Self::ParameterSetId => "parameter set",
            Self::ModelVersion => "model version",
            Self::HardwareVersion => "hardware version",
            Self::FirmwareVersion => "firmware version",
            Self::SoftwareVersion => "software version",
            Self::CustomerLocation => "customer location",
            Self::Customer => "customer",
            Self::AccessCodeUser => "access code (user)",
            Self::AccessCodeOperator => "access code (operator)",
            Self::AccessCodeSystemOperator => "access code (system operator)",
            Self::AccessCodeDeveloper => "access code (developer)",
            Self::Password => "password",
            Self::ErrorFlags => "error flags",
            Self::ErrorMask => "error mask",
            Self::SecurityKey => "security key",
            Self::DigitalOutput => "digital output",
            Self::DigitalInput => "digital input",
            Self::BaudRate => "baud rate",
            Self::ResponseDelay => "response delay",
            Self::Retry => "retry",
            Self::RemoteControl => "remote control",
            Self::FirstStorageNumber => "first storage number",
            Self::LastStorageNumber => "last storage number",
            Self::StorageBlockSize => "storage block size",
            Self::TariffDescriptor => "tariff and subunit descriptor",
            Self::StorageInterval => "storage interval",
            Self::OperatorSpecific => "operator specific",
            Self::TimePointSecond => "time point (second)",
            Self::DurationSinceReadout => "duration since last readout",
            Self::TariffStart => "tariff start",
            Self::TariffDuration => "tariff duration",
            Self::TariffPeriod => "tariff period",
            Self::Dimensionless => "dimensionless",
            Self::WmbusContainer => "wireless M-Bus container",
            Self::TransmissionPeriod => "transmission period",
            Self::Voltage => "voltage",
            Self::Current => "current",
            Self::ResetCounter => "reset counter",
            Self::CumulationCounter => "cumulation counter",
            Self::ControlSignal => "control signal",
            Self::DayOfWeek => "day of week",
            Self::WeekNumber => "week number",
            Self::DayChange => "day change",
            Self::ParameterActivation => "parameter activation",
            Self::SupplierInformation => "supplier information",
            Self::DurationSinceCumulation => "duration since last cumulation",
            Self::BatteryOperatingTime => "battery operating time",
            Self::BatteryChange => "battery change",
            Self::SignalLevel => "signal level",
            Self::DaylightSaving => "daylight saving",
            Self::ListeningWindow => "listening window",
            Self::RemainingBatteryLife => "remaining battery life",
            Self::StopCount => "stop count",
            Self::ManufacturerContainer => "manufacturer container",
            Self::ReactiveEnergy => "reactive energy",
            Self::ApparentEnergy => "apparent energy",
            Self::ReactivePower => "reactive power",
            Self::ApparentPower => "apparent power",
            Self::RelativeHumidity => "relative humidity",
            Self::PhaseVoltageVoltage => "phase U-U",
            Self::PhaseVoltageCurrent => "phase U-I",
            Self::Frequency => "frequency",
            Self::TemperatureLimit => "temperature limit",
            Self::CumulativeMaxPower => "cumulative max power",
            Self::SelectedApplication => "selected application",
            Self::SubDeviceTypes => "sub-device types",
            Self::CarbonDioxide => "carbon dioxide",
            Self::CarbonMonoxide => "carbon monoxide",
            Self::VolatileOrganicCompounds => "volatile organic compounds",
            Self::Particles(Particles::Any) => "particles",
            Self::Particles(Particles::Pm1) => "particles PM1",
            Self::Particles(Particles::Pm2_5) => "particles PM2.5",
            Self::Particles(Particles::Pm10) => "particles PM10",
            Self::Illuminance => "illuminance",
            Self::LuminousIntensity => "luminous intensity",
            Self::Irradiance => "irradiance",
            Self::WindSpeed => "wind speed",
            Self::Rainfall => "rainfall",
            Self::SoundLevel => "sound level",
            Self::Turbidity => "turbidity",
            Self::Ph => "pH",
            Self::Events(e) => e.name(),
            Self::BatteryStatus => "battery status",
            Self::ChamberPollution => "chamber pollution",
            Self::Distance => "distance",
            Self::Moisture => "moisture",
            Self::StatusBits(s) => s.name(),
            Self::Reserved { .. } => "reserved",
            Self::MissingExtension(_) => "extension table without code",
        }
    }

    /// Whether a value of this kind is a point in time, decoded from its size (2 bytes:
    /// type G, 3: type J, 4: type F, 6: type I).
    pub(crate) const fn is_time_point(self) -> bool {
        matches!(
            self,
            Self::Date | Self::DateTime | Self::TariffStart | Self::BatteryChange | Self::DayChange
        )
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reserved { table, code } => write!(f, "reserved {table:?} code 0x{code:02x}"),
            other => f.write_str(other.name()),
        }
    }
}

/// Particle size range (`FD FD 17`–`1E`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Particles {
    /// Unspecified range.
    Any,
    /// PM1.
    Pm1,
    /// PM2.5.
    Pm2_5,
    /// PM10.
    Pm10,
}

/// What a sensor event counter counts (`FD FD 2C`–`34`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// Dismounts / removals.
    Dismounts,
    /// Test button operations.
    TestButton,
    /// Alarms.
    Alarms,
    /// Alarm mute operations.
    AlarmMutes,
    /// Obstacles detected.
    Obstacles,
    /// Smoke entries blocked.
    SmokeEntryBlocked,
    /// Smoke chamber defects.
    ChamberDefects,
    /// Self-test failures.
    SelfTestFailures,
    /// Sounder defects.
    SounderDefects,
}

impl Event {
    const fn name(self) -> &'static str {
        match self {
            Self::Dismounts => "dismounts",
            Self::TestButton => "test button operations",
            Self::Alarms => "alarms",
            Self::AlarmMutes => "alarm mutes",
            Self::Obstacles => "obstacles detected",
            Self::SmokeEntryBlocked => "smoke entries blocked",
            Self::ChamberDefects => "smoke chamber defects",
            Self::SelfTestFailures => "self-test failures",
            Self::SounderDefects => "sounder defects",
        }
    }
}

/// Which sensor a status-bits record belongs to (`FD FD 40`–`44`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sensor {
    /// Pressure sensor.
    Pressure,
    /// Smoke alarm.
    Smoke,
    /// CO alarm.
    CarbonMonoxide,
    /// Heat alarm.
    Heat,
    /// Door / window sensor.
    DoorWindow,
}

impl Sensor {
    const fn name(self) -> &'static str {
        match self {
            Self::Pressure => "pressure sensor status",
            Self::Smoke => "smoke alarm status",
            Self::CarbonMonoxide => "CO alarm status",
            Self::Heat => "heat alarm status",
            Self::DoorWindow => "door/window sensor status",
        }
    }
}

/// What a value information block says about the value: what it is, and its unit and
/// decimal exponent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Description {
    /// What the value is.
    pub kind: Kind,
    /// The unit, for physical quantities and durations; `None` for identifiers,
    /// counters, flags, dates and anything not interpreted.
    pub unit: Option<Unit>,
    /// The decimal exponent: the value is `raw × 10^exponent` in `unit`.
    pub exponent: i8,
    /// The table the code came from.
    pub table: Table,
}

impl fmt::Display for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.kind, f)
    }
}

/// Which way a compact profile runs (EN 13757-3:2018 Annex F.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `1F`: elements follow the base value forward in time.
    Forward,
    /// `13`: elements go back in time from the base value.
    Inverse,
    /// `1E`: forward, with register (storage) numbers: OMS billing history.
    WithRegisters,
}

/// The denominator of a "per" VIFE.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Per {
    /// Per second.
    Second,
    /// Per minute.
    Minute,
    /// Per hour.
    Hour,
    /// Per day.
    Day,
    /// Per week.
    Week,
    /// Per month.
    Month,
    /// Per year.
    Year,
    /// Per revolution / measurement.
    Revolution,
    /// Per litre.
    Litre,
    /// Per m³.
    CubicMetre,
    /// Per kg.
    Kilogram,
    /// Per K.
    Kelvin,
    /// Per kWh.
    KilowattHour,
    /// Per GJ.
    Gigajoule,
    /// Per kW.
    Kilowatt,
    /// Per K·l.
    KelvinLitre,
    /// Per V.
    Volt,
    /// Per A.
    Ampere,
}

impl Per {
    /// The unit symbol of the denominator, e.g. `"h"`.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Second => "s",
            Self::Minute => "min",
            Self::Hour => "h",
            Self::Day => "d",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::Revolution => "revolution",
            Self::Litre => "l",
            Self::CubicMetre => "m³",
            Self::Kilogram => "kg",
            Self::Kelvin => "K",
            Self::KilowattHour => "kWh",
            Self::Gigajoule => "GJ",
            Self::Kilowatt => "kW",
            Self::KelvinLitre => "K·l",
            Self::Volt => "V",
            Self::Ampere => "A",
        }
    }
}

/// What a value is multiplied by (VIFE `36`–`38`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Times {
    /// × s.
    Second,
    /// × s/V.
    SecondPerVolt,
    /// × s/A.
    SecondPerAmpere,
}

/// A record error code, sent by a slave in a combinable VIFE `00`–`1F`
/// (EN 13757-3, Table 19).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecordError(pub u8);

impl RecordError {
    /// A short English description, if the code is defined.
    pub const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0x00 => "none",
            0x01 => "too many DIFEs",
            0x02 => "storage number not implemented",
            0x03 => "unit number not implemented",
            0x04 => "tariff number not implemented",
            0x05 => "function not implemented",
            0x06 => "data class not implemented",
            0x07 => "data size not implemented",
            0x0B => "too many VIFEs",
            0x0C => "illegal VIF group",
            0x0D => "illegal VIF exponent",
            0x0E => "VIF/DIF mismatch",
            0x0F => "unimplemented action",
            0x15 => "no data available",
            0x16 => "data overflow",
            0x17 => "data underflow",
            0x18 => "data error",
            0x1C => "premature end of record",
            _ => return None,
        })
    }
}

/// One combinable (orthogonal) VIFE: how it modifies the value (EN 13757-3, Table 15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Modifier {
    /// `00`–`11`, `15`–`1C`: the slave reports an error for this record.
    Error(RecordError),
    /// `12`: an average value.
    Average,
    /// `13`, `1E`, `1F`: the record is a compact profile (see
    /// [`Record::profile`](crate::Record::profile)).
    CompactProfile(Direction),
    /// `14`: a relative deviation.
    RelativeDeviation,
    /// `1D`: standard-conformant data content (e.g. error flags as defined by OMS).
    StandardConformant,
    /// `20`–`27`, `2C`–`35`: the value is per this denominator.
    Per(Per),
    /// `28`–`2B`: increment per input or output pulse on channel 0 or 1.
    IncrementPerPulse {
        /// Output (`true`) or input channel.
        output: bool,
        /// Channel 0 or 1.
        channel: u8,
    },
    /// `36`–`38`: the value is multiplied by this.
    MultipliedBy(Times),
    /// `39`: the value is the start date (and time) of what the VIF describes.
    StartDateOf,
    /// `3A`: the VIF is the uncorrected unit (e.g. volume at measuring conditions).
    UncorrectedUnit,
    /// `3B`: forward flow: accumulation of positive contributions only.
    Forward,
    /// `3C`: backward flow: accumulation of the absolute value of negative
    /// contributions only.
    Backward,
    /// `3D`: an alternate, non-metric unit system.
    NonMetric,
    /// `3E`: the value at base conditions.
    BaseConditions,
    /// `3F`: OBIS declaration.
    ObisDeclaration,
    /// `40` / `48`: a lower or upper limit value.
    Limit {
        /// Upper (`true`) or lower limit.
        upper: bool,
    },
    /// `41` / `49`: the number of times the lower or upper limit was exceeded.
    ExceedCount {
        /// Upper (`true`) or lower limit.
        upper: bool,
    },
    /// `42`–`4F` (`E100 uf1b`): the date (and time) a limit exceed began or ended.
    LimitExceedDate {
        /// Upper (`true`) or lower limit.
        upper: bool,
        /// Last (`true`) or first exceed.
        last: bool,
        /// End (`true`) or beginning.
        end: bool,
    },
    /// `50`–`5F` (`E101 ufnn`): how long a limit was exceeded.
    LimitExceedDuration {
        /// Upper (`true`) or lower limit.
        upper: bool,
        /// Last (`true`) or first exceed.
        last: bool,
        /// Seconds, minutes, hours or days.
        unit: Unit,
    },
    /// `60`–`67` (`E110 0fnn`): a duration.
    Duration {
        /// Last (`true`) or first.
        last: bool,
        /// Seconds, minutes, hours or days.
        unit: Unit,
    },
    /// `68` / `6C`: the value during a lower or upper limit exceed.
    ValueDuringLimitExceed {
        /// Upper (`true`) or lower limit.
        upper: bool,
    },
    /// `69`: leakage values.
    Leakage,
    /// `6D`: overflow values.
    Overflow,
    /// `6A`, `6B`, `6E`, `6F` (`E110 1f1b`): the date (and time) of a first or last
    /// beginning or end.
    DateOf {
        /// Last (`true`) or first.
        last: bool,
        /// End (`true`) or beginning.
        end: bool,
    },
    /// `70`–`77` (×10^(nnn−6)) and `7D` (×10³): multiplicative correction, applied by
    /// [`Record::quantity`](crate::Record::quantity).
    Scale(i8),
    /// `78`–`7B`: the record is an additive correction constant (offset) of
    /// 10^(nn−3) × the VIF's unit.
    Offset(i8),
    /// `FC` and the code after it: from the extension of the combinable table.
    Extension(Extension),
    /// `7E`: a future value.
    FutureValue,
    /// `7F`: this and every following VIFE (and the data) are manufacturer-specific.
    ManufacturerSpecific,
    /// A code the table reserves.
    Reserved(u8),
}

impl Modifier {
    /// Whether [`Record::quantity`](crate::Record::quantity) may still report the value
    /// in the VIF's unit: the modifier scales it (applied) or annotates it without
    /// changing unit or meaning.
    pub const fn keeps_quantity(self) -> bool {
        matches!(
            self,
            Self::Scale(_)
                | Self::Error(RecordError(0))
                | Self::UncorrectedUnit
                | Self::Forward
                | Self::Backward
                | Self::Average
                | Self::StandardConformant
                | Self::BaseConditions
                | Self::Limit { .. }
                | Self::ValueDuringLimitExceed { .. }
                | Self::Leakage
                | Self::Overflow
                | Self::FutureValue
        ) || matches!(self, Self::Extension(e) if e.keeps_quantity())
    }

    /// Whether the value becomes a point in time (and is decoded as a date).
    pub(crate) const fn is_time_point(self) -> bool {
        matches!(self, Self::StartDateOf | Self::LimitExceedDate { .. } | Self::DateOf { .. })
    }

    #[allow(clippy::cast_possible_wrap)] // every code here is masked to 7 bits
    const fn from_code(code: u8) -> Self {
        let c = code & 0x7F;
        let upper = c & 0x08 != 0;
        let last = c & 0x04 != 0;
        match c {
            0x12 => Self::Average,
            0x13 => Self::CompactProfile(Direction::Inverse),
            0x1E => Self::CompactProfile(Direction::WithRegisters),
            0x1F => Self::CompactProfile(Direction::Forward),
            0x14 => Self::RelativeDeviation,
            0x1D => Self::StandardConformant,
            0x00..=0x1C => Self::Error(RecordError(c)),
            0x20 => Self::Per(Per::Second),
            0x21 => Self::Per(Per::Minute),
            0x22 => Self::Per(Per::Hour),
            0x23 => Self::Per(Per::Day),
            0x24 => Self::Per(Per::Week),
            0x25 => Self::Per(Per::Month),
            0x26 => Self::Per(Per::Year),
            0x27 => Self::Per(Per::Revolution),
            0x28..=0x2B => Self::IncrementPerPulse { output: c & 0x02 != 0, channel: c & 0x01 },
            0x2C => Self::Per(Per::Litre),
            0x2D => Self::Per(Per::CubicMetre),
            0x2E => Self::Per(Per::Kilogram),
            0x2F => Self::Per(Per::Kelvin),
            0x30 => Self::Per(Per::KilowattHour),
            0x31 => Self::Per(Per::Gigajoule),
            0x32 => Self::Per(Per::Kilowatt),
            0x33 => Self::Per(Per::KelvinLitre),
            0x34 => Self::Per(Per::Volt),
            0x35 => Self::Per(Per::Ampere),
            0x36 => Self::MultipliedBy(Times::Second),
            0x37 => Self::MultipliedBy(Times::SecondPerVolt),
            0x38 => Self::MultipliedBy(Times::SecondPerAmpere),
            0x39 => Self::StartDateOf,
            0x3A => Self::UncorrectedUnit,
            0x3B => Self::Forward,
            0x3C => Self::Backward,
            0x3D => Self::NonMetric,
            0x3E => Self::BaseConditions,
            0x3F => Self::ObisDeclaration,
            0x40 | 0x48 => Self::Limit { upper },
            0x41 | 0x49 => Self::ExceedCount { upper },
            0x42 | 0x43 | 0x46 | 0x47 | 0x4A | 0x4B | 0x4E | 0x4F => {
                Self::LimitExceedDate { upper, last, end: c & 0x01 != 0 }
            }
            0x50..=0x5F => Self::LimitExceedDuration { upper, last, unit: Unit::duration(c) },
            0x60..=0x67 => Self::Duration { last, unit: Unit::duration(c) },
            0x68 | 0x6C => Self::ValueDuringLimitExceed { upper: c & 0x04 != 0 },
            0x69 => Self::Leakage,
            0x6D => Self::Overflow,
            0x6A | 0x6B | 0x6E | 0x6F => Self::DateOf { last, end: c & 0x01 != 0 },
            0x70..=0x77 => Self::Scale((c & 0x07) as i8 - 6),
            0x78..=0x7B => Self::Offset((c & 0x03) as i8 - 3),
            0x7D => Self::Scale(3),
            0x7E => Self::FutureValue,
            0x7F => Self::ManufacturerSpecific,
            _ => Self::Reserved(c),
        }
    }

    /// A short English description, e.g. `"per hour"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Error(_) => "error",
            Self::Average => "average",
            Self::CompactProfile(_) => "compact profile",
            Self::RelativeDeviation => "relative deviation",
            Self::StandardConformant => "standard conformant",
            Self::Per(Per::Second) => "per second",
            Self::Per(Per::Minute) => "per minute",
            Self::Per(Per::Hour) => "per hour",
            Self::Per(Per::Day) => "per day",
            Self::Per(Per::Week) => "per week",
            Self::Per(Per::Month) => "per month",
            Self::Per(Per::Year) => "per year",
            Self::Per(Per::Revolution) => "per revolution",
            Self::Per(_) => "per unit",
            Self::IncrementPerPulse { output: false, .. } => "increment per input pulse",
            Self::IncrementPerPulse { output: true, .. } => "increment per output pulse",
            Self::MultipliedBy(_) => "multiplied by",
            Self::StartDateOf => "start date of",
            Self::UncorrectedUnit => "uncorrected",
            Self::Forward => "forward",
            Self::Backward => "backward",
            Self::NonMetric => "non-metric",
            Self::BaseConditions => "at base conditions",
            Self::ObisDeclaration => "OBIS declaration",
            Self::Limit { upper: false } => "lower limit",
            Self::Limit { upper: true } => "upper limit",
            Self::ExceedCount { upper: false } => "lower limit exceeds",
            Self::ExceedCount { upper: true } => "upper limit exceeds",
            Self::LimitExceedDate { .. } => "date of limit exceed",
            Self::LimitExceedDuration { .. } => "duration of limit exceed",
            Self::Duration { .. } => "duration",
            Self::ValueDuringLimitExceed { upper: false } => "during lower limit exceed",
            Self::ValueDuringLimitExceed { upper: true } => "during upper limit exceed",
            Self::Leakage => "leakage",
            Self::Overflow => "overflow",
            Self::DateOf { .. } => "date of",
            Self::Scale(_) => "corrected",
            Self::Offset(_) => "additive correction",
            Self::Extension(_) => "extended modifier",
            Self::FutureValue => "future value",
            Self::ManufacturerSpecific => "manufacturer specific",
            Self::Reserved(_) => "reserved modifier",
        }
    }
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first_last = |last: bool| if last { "last" } else { "first" };
        let lower_upper = |upper: bool| if upper { "upper" } else { "lower" };
        match *self {
            Self::Error(e) => match e.name() {
                Some(n) => write!(f, "error: {n}"),
                None => write!(f, "error 0x{:02x}", e.0),
            },
            Self::Per(p) if !matches!(p, Per::Second | Per::Minute | Per::Hour | Per::Day) => {
                write!(f, "per {}", p.symbol())
            }
            Self::IncrementPerPulse { output, channel } => {
                write!(
                    f,
                    "increment per {} pulse, channel {channel}",
                    if output { "output" } else { "input" }
                )
            }
            Self::MultipliedBy(t) => f.write_str(match t {
                Times::Second => "× s",
                Times::SecondPerVolt => "× s/V",
                Times::SecondPerAmpere => "× s/A",
            }),
            Self::LimitExceedDate { upper, last, end } => write!(
                f,
                "{} of {} {} limit exceed",
                if end { "end" } else { "begin" },
                first_last(last),
                lower_upper(upper)
            ),
            Self::LimitExceedDuration { upper, last, unit } => {
                write!(f, "duration of {} {} limit exceed ({unit})", first_last(last), lower_upper(upper))
            }
            Self::Duration { last, unit } => write!(f, "duration of {} ({unit})", first_last(last)),
            Self::DateOf { last, end } => {
                write!(f, "date of {} {}", first_last(last), if end { "end" } else { "begin" })
            }
            Self::Scale(e) => write!(f, "× 10^{e}"),
            Self::Offset(e) => write!(f, "additive correction × 10^{e}"),
            Self::Extension(e) => write!(f, "{e}"),
            Self::Reserved(c) => write!(f, "reserved modifier 0x{c:02x}"),
            other => f.write_str(other.name()),
        }
    }
}

/// A code from the extension of the combinable VIFE table (after combinable `FC`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Extension {
    /// At phase L1, L2 or L3 (`01`–`03`).
    Phase(u8),
    /// At neutral (`04`).
    Neutral,
    /// Between phases L1–L2, L2–L3, L3–L1 (`05`–`07`), given as the first phase.
    BetweenPhases(u8),
    /// In quadrant Q1–Q4 (`08`–`0B`).
    Quadrant(u8),
    /// Delta between import and export (`0C`).
    ImportExportDelta,
    /// Accumulation of the absolute value of positive and negative contributions (`10`).
    Absolute,
    /// The data is unsigned (data type C, `11`): applied when the value is decoded.
    Unsigned,
    /// The data is boolean (data type D, `12`).
    Boolean,
    /// A code this crate does not interpret.
    Other(u8),
}

impl Extension {
    const fn from_code(c: u8) -> Self {
        match c {
            0x01..=0x03 => Self::Phase(c),
            0x04 => Self::Neutral,
            0x05..=0x07 => Self::BetweenPhases(c - 4),
            0x08..=0x0B => Self::Quadrant(c - 7),
            0x0C => Self::ImportExportDelta,
            0x10 => Self::Absolute,
            0x11 => Self::Unsigned,
            0x12 => Self::Boolean,
            other => Self::Other(other),
        }
    }

    const fn keeps_quantity(self) -> bool {
        !matches!(self, Self::Boolean | Self::Other(_))
    }
}

impl fmt::Display for Extension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Phase(p) => write!(f, "phase L{p}"),
            Self::Neutral => f.write_str("neutral"),
            Self::BetweenPhases(p) => write!(f, "phase L{p}–L{}", p % 3 + 1),
            Self::Quadrant(q) => write!(f, "quadrant Q{q}"),
            Self::ImportExportDelta => f.write_str("import − export"),
            Self::Absolute => f.write_str("absolute"),
            Self::Unsigned => f.write_str("unsigned"),
            Self::Boolean => f.write_str("boolean"),
            Self::Other(c) => write!(f, "extended modifier 0x{c:02x}"),
        }
    }
}

/// The combinable VIFEs of a [`Vib`], as [`Modifier`]s.
#[derive(Clone, Debug)]
pub struct Modifiers<'v> {
    rest: &'v [u8],
    manufacturer: bool,
}

impl Iterator for Modifiers<'_> {
    type Item = Modifier;

    fn next(&mut self) -> Option<Modifier> {
        if self.manufacturer {
            return None;
        }
        let (&code, rest) = self.rest.split_first()?;
        self.rest = rest;
        let m = match code & 0x7F {
            0x7C => {
                let (&next, rest) = self.rest.split_first().unwrap_or((&0, &[]));
                self.rest = rest;
                Modifier::Extension(Extension::from_code(next & 0x7F))
            }
            c => Modifier::from_code(c),
        };
        if m == Modifier::ManufacturerSpecific {
            self.manufacturer = true;
        }
        Some(m)
    }
}

impl<'a> Vib<'a> {
    pub(crate) const fn new(
        vif: u8,
        vifes: [u8; MAX_EXTENSIONS],
        vife_len: u8,
        plain_text: Option<&'a [u8]>,
    ) -> Self {
        Self { vif, vifes, vife_len, plain_text }
    }

    /// The VIF byte, as transmitted.
    pub const fn vif(&self) -> u8 {
        self.vif
    }

    /// The VIFE bytes, as transmitted.
    pub fn vifes(&self) -> &[u8] {
        self.vifes.get(..usize::from(self.vife_len)).unwrap_or(&[])
    }

    /// For a plain-text VIF (`7C`/`FC`): the unit text, as transmitted (M-Bus sends it
    /// last character first).
    pub const fn plain_text(&self) -> Option<&'a [u8]> {
        self.plain_text
    }

    /// The extension table this VIF selects, and how many VIFEs name the code in it
    /// (`FD FD xx` takes two).
    fn table(&self) -> Option<(Table, usize)> {
        match (self.vif & 0x7F, self.vifes().first().map(|c| c & 0x7F)) {
            (0x7B, _) => Some((Table::Fb, 1)),
            (0x7D, Some(0x7D)) => Some((Table::FdSecond, 2)),
            (0x7D, _) => Some((Table::Fd, 1)),
            (0x6F, _) => Some((Table::Third, 1)),
            _ => None,
        }
    }

    /// What the value is: kind, unit and decimal exponent, from the primary table or
    /// the `FB`/`FD` extension table. Combinable VIFEs are in [`Vib::modifiers`].
    pub fn description(&self) -> Description {
        let Some((table, len)) = self.table() else { return primary(self.vif & 0x7F) };
        let Some(&code) = self.vifes().get(len - 1) else {
            return d(Kind::MissingExtension(table), None, 0, table);
        };
        let code = code & 0x7F;
        match table {
            Table::Fb => fb(code),
            Table::Fd => fd(code),
            Table::FdSecond => fd_second(code),
            Table::Primary | Table::Third => d(Kind::Reserved { table, code }, None, 0, table),
        }
    }

    /// The combinable VIFEs, as modifiers. After a manufacturer-specific VIF (`7F`/`FF`)
    /// or a `7F` VIFE, the remaining bytes are not interpreted.
    pub fn modifiers(&self) -> Modifiers<'_> {
        let vifes = self.vifes();
        let skip = self.table().map_or(0, |(_, len)| len);
        let rest = vifes.get(skip..).unwrap_or(&[]);
        Modifiers { rest, manufacturer: self.vif & 0x7F == 0x7F }
    }
}

const fn d(kind: Kind, unit: Option<Unit>, exponent: i8, table: Table) -> Description {
    Description { kind, unit, exponent, table }
}

/// `n` (the low 3 bits) minus `offset`, or the low 2 bits with `two` set.
#[allow(clippy::cast_possible_wrap)] // masked to at most 4 bits
const fn exp(code: u8, mask: u8, offset: i8) -> i8 {
    (code & mask) as i8 - offset
}

/// The primary VIF table (EN 13757-3:2018, Table 10).
fn primary(v: u8) -> Description {
    use Kind as K;
    const P: Table = Table::Primary;
    let q = |kind, unit, e| d(kind, Some(unit), e, P);
    match v {
        0x00..=0x07 => q(K::Energy, Unit::Wh, exp(v, 7, 3)),
        0x08..=0x0F => q(K::Energy, Unit::J, exp(v, 7, 0)),
        0x10..=0x17 => q(K::Volume, Unit::M3, exp(v, 7, 6)),
        0x18..=0x1F => q(K::Mass, Unit::Kg, exp(v, 7, 3)),
        0x20..=0x23 => q(K::OnTime, Unit::duration(v), 0),
        0x24..=0x27 => q(K::OperatingTime, Unit::duration(v), 0),
        0x28..=0x2F => q(K::Power, Unit::W, exp(v, 7, 3)),
        0x30..=0x37 => q(K::Power, Unit::JPerH, exp(v, 7, 0)),
        0x38..=0x3F => q(K::VolumeFlow, Unit::M3PerH, exp(v, 7, 6)),
        0x40..=0x47 => q(K::VolumeFlow, Unit::M3PerMin, exp(v, 7, 7)),
        0x48..=0x4F => q(K::VolumeFlow, Unit::M3PerS, exp(v, 7, 9)),
        0x50..=0x57 => q(K::MassFlow, Unit::KgPerH, exp(v, 7, 3)),
        0x58..=0x5B => q(K::FlowTemperature, Unit::Celsius, exp(v, 3, 3)),
        0x5C..=0x5F => q(K::ReturnTemperature, Unit::Celsius, exp(v, 3, 3)),
        0x60..=0x63 => q(K::TemperatureDifference, Unit::Kelvin, exp(v, 3, 3)),
        0x64..=0x67 => q(K::ExternalTemperature, Unit::Celsius, exp(v, 3, 3)),
        0x68..=0x6B => q(K::Pressure, Unit::Bar, exp(v, 3, 3)),
        0x6C => d(K::Date, None, 0, P),
        0x6D => d(K::DateTime, None, 0, P),
        0x6E => d(K::HcaUnits, None, 0, P),
        0x70..=0x73 => q(K::AveragingDuration, Unit::duration(v), 0),
        0x74..=0x77 => q(K::ActualityDuration, Unit::duration(v), 0),
        0x78 => d(K::FabricationNumber, None, 0, P),
        0x79 => d(K::Identification, None, 0, P),
        0x7A => d(K::BusAddress, None, 0, P),
        0x7C => d(K::PlainText, None, 0, P),
        0x7E => d(K::AnyVif, None, 0, P),
        0x7F => d(K::ManufacturerSpecific, None, 0, P),
        // 0x6F; 0x7B and 0x7D only reach here without a table code.
        _ => d(K::Reserved { table: P, code: v }, None, 0, P),
    }
}

/// The extension table after VIF `FD` (EN 13757-3:2018, Table 12).
#[allow(clippy::too_many_lines)]
fn fd(c: u8) -> Description {
    use Kind as K;
    const T: Table = Table::Fd;
    let n = |kind| d(kind, None, 0, T);
    let q = |kind, unit, e| d(kind, Some(unit), e, T);
    match c {
        0x00..=0x03 => q(K::Credit, Unit::Currency, exp(c, 3, 3)),
        0x04..=0x07 => q(K::Debit, Unit::Currency, exp(c, 3, 3)),
        0x08 => n(K::AccessNumber),
        0x09 => n(K::Medium),
        0x0A => n(K::Manufacturer),
        0x0B => n(K::ParameterSetId),
        0x0C => n(K::ModelVersion),
        0x0D => n(K::HardwareVersion),
        0x0E => n(K::FirmwareVersion),
        0x0F => n(K::SoftwareVersion),
        0x10 => n(K::CustomerLocation),
        0x11 => n(K::Customer),
        0x12 => n(K::AccessCodeUser),
        0x13 => n(K::AccessCodeOperator),
        0x14 => n(K::AccessCodeSystemOperator),
        0x15 => n(K::AccessCodeDeveloper),
        0x16 => n(K::Password),
        0x17 => n(K::ErrorFlags),
        0x18 => n(K::ErrorMask),
        0x19 => n(K::SecurityKey),
        0x1A => n(K::DigitalOutput),
        0x1B => n(K::DigitalInput),
        0x1C => q(K::BaudRate, Unit::Baud, 0),
        0x1D => q(K::ResponseDelay, Unit::BitTimes, 0),
        0x1E => n(K::Retry),
        0x1F => n(K::RemoteControl),
        0x20 => n(K::FirstStorageNumber),
        0x21 => n(K::LastStorageNumber),
        0x22 => n(K::StorageBlockSize),
        0x23 => n(K::TariffDescriptor),
        0x24..=0x27 => q(K::StorageInterval, Unit::duration(c), 0),
        0x28 => q(K::StorageInterval, Unit::Months, 0),
        0x29 => q(K::StorageInterval, Unit::Years, 0),
        0x2A => n(K::OperatorSpecific),
        0x2B => q(K::TimePointSecond, Unit::Seconds, 0),
        0x2C..=0x2F => q(K::DurationSinceReadout, Unit::duration(c), 0),
        0x30 => n(K::TariffStart),
        0x31..=0x33 => q(K::TariffDuration, Unit::duration(c), 0),
        0x34..=0x37 => q(K::TariffPeriod, Unit::duration(c), 0),
        0x38 => q(K::TariffPeriod, Unit::Months, 0),
        0x39 => q(K::TariffPeriod, Unit::Years, 0),
        0x3A => n(K::Dimensionless),
        0x3B => n(K::WmbusContainer),
        0x3C..=0x3F => q(K::TransmissionPeriod, Unit::duration(c), 0),
        0x40..=0x4F => q(K::Voltage, Unit::Volt, exp(c, 0x0F, 9)),
        0x50..=0x5F => q(K::Current, Unit::Ampere, exp(c, 0x0F, 12)),
        0x60 => n(K::ResetCounter),
        0x61 => n(K::CumulationCounter),
        0x62 => n(K::ControlSignal),
        0x63 => n(K::DayOfWeek),
        0x64 => n(K::WeekNumber),
        0x65 => n(K::DayChange),
        0x66 => n(K::ParameterActivation),
        0x67 => n(K::SupplierInformation),
        0x68..=0x6B => q(K::DurationSinceCumulation, Unit::long_duration(c), 0),
        0x6C..=0x6F => q(K::BatteryOperatingTime, Unit::long_duration(c), 0),
        0x70 => n(K::BatteryChange),
        0x71 => q(K::SignalLevel, Unit::Dbm, 0),
        0x72 => n(K::DaylightSaving),
        0x73 => n(K::ListeningWindow),
        0x74 => q(K::RemainingBatteryLife, Unit::Days, 0),
        0x75 => n(K::StopCount),
        0x76 => n(K::ManufacturerContainer),
        _ => n(K::Reserved { table: T, code: c }),
    }
}

/// The extension table after VIF `FB` (EN 13757-3:2018, Table 14).
fn fb(c: u8) -> Description {
    use Kind as K;
    const T: Table = Table::Fb;
    let q = |kind, unit, e| d(kind, Some(unit), e, T);
    match c {
        // MWh × 10^(n−1) = Wh × 10^(n+5)
        0x00 | 0x01 => q(K::Energy, Unit::Wh, exp(c, 1, -5)),
        // kvarh × 10^(n−?) per EN 13757-3:2018
        0x02 | 0x03 => q(K::ReactiveEnergy, Unit::VarH, exp(c, 1, -3)),
        0x04 | 0x05 => q(K::ApparentEnergy, Unit::VaH, exp(c, 1, -3)),
        // GJ × 10^(n−1) = J × 10^(n+8)
        0x08 | 0x09 => q(K::Energy, Unit::J, exp(c, 1, -8)),
        // Mcal × 10^(n−1) = cal × 10^(n+5)
        0x0C..=0x0F => q(K::Energy, Unit::Calorie, exp(c, 3, -5)),
        // m³ × 10^(n+2)
        0x10 | 0x11 => q(K::Volume, Unit::M3, exp(c, 1, -2)),
        0x14..=0x17 => q(K::ReactivePower, Unit::Var, exp(c, 3, 0)),
        // t × 10^(n+2) = kg × 10^(n+5)
        0x18 | 0x19 => q(K::Mass, Unit::Kg, exp(c, 1, -5)),
        0x1A | 0x1B => q(K::RelativeHumidity, Unit::Percent, exp(c, 1, 1)),
        0x20 => q(K::Volume, Unit::CubicFeet, 0),
        0x21 => q(K::Volume, Unit::CubicFeet, -1),
        0x22 => q(K::Volume, Unit::UsGallon, -1),
        0x23 => q(K::Volume, Unit::UsGallon, 0),
        0x24 => q(K::VolumeFlow, Unit::UsGallonPerMin, -3),
        0x25 => q(K::VolumeFlow, Unit::UsGallonPerMin, 0),
        0x26 => q(K::VolumeFlow, Unit::UsGallonPerH, 0),
        // MW × 10^(n−1) = W × 10^(n+5)
        0x28 | 0x29 => q(K::Power, Unit::W, exp(c, 1, -5)),
        0x2A => q(K::PhaseVoltageVoltage, Unit::Degree, -1),
        0x2B => q(K::PhaseVoltageCurrent, Unit::Degree, -1),
        0x2C..=0x2F => q(K::Frequency, Unit::Hertz, exp(c, 3, 3)),
        // GJ/h × 10^(n−1) = J/h × 10^(n+8)
        0x30 | 0x31 => q(K::Power, Unit::JPerH, exp(c, 1, -8)),
        0x34..=0x37 => q(K::ApparentPower, Unit::VoltAmpere, exp(c, 3, 0)),
        0x58..=0x5B => q(K::FlowTemperature, Unit::Fahrenheit, exp(c, 3, 3)),
        0x5C..=0x5F => q(K::ReturnTemperature, Unit::Fahrenheit, exp(c, 3, 3)),
        0x60..=0x63 => q(K::TemperatureDifference, Unit::Fahrenheit, exp(c, 3, 3)),
        0x64..=0x67 => q(K::ExternalTemperature, Unit::Fahrenheit, exp(c, 3, 3)),
        0x70..=0x73 => q(K::TemperatureLimit, Unit::Fahrenheit, exp(c, 3, 3)),
        0x74..=0x77 => q(K::TemperatureLimit, Unit::Celsius, exp(c, 3, 3)),
        0x78..=0x7F => q(K::CumulativeMaxPower, Unit::W, exp(c, 7, 3)),
        _ => d(K::Reserved { table: T, code: c }, None, 0, T),
    }
}

/// The second-level table after `FD FD` (OMS Vol. 2 Annex B, citing prEN 13757-3:2023).
fn fd_second(c: u8) -> Description {
    use Kind as K;
    const T: Table = Table::FdSecond;
    let n = |kind| d(kind, None, 0, T);
    let q = |kind, unit, e| d(kind, Some(unit), e, T);
    match c {
        0x00 => n(K::SelectedApplication),
        0x01 => n(K::SubDeviceTypes),
        0x02 => q(K::RemainingBatteryLife, Unit::Months, 0),
        0x10 | 0x11 => q(K::CarbonDioxide, Unit::Ppm, exp(c, 1, 0)),
        0x12 | 0x13 => q(K::CarbonMonoxide, Unit::Ppm, exp(c, 1, 0)),
        0x14 | 0x15 => q(K::VolatileOrganicCompounds, Unit::Ppb, exp(c, 1, 0)),
        0x16 => q(K::VolatileOrganicCompounds, Unit::MicrogramPerM3, 0),
        0x17 => q(K::Particles(Particles::Any), Unit::MicrogramPerM3, 0),
        0x18 => q(K::Particles(Particles::Pm1), Unit::MicrogramPerM3, 0),
        0x19 => q(K::Particles(Particles::Pm2_5), Unit::MicrogramPerM3, 0),
        0x1A => q(K::Particles(Particles::Pm10), Unit::MicrogramPerM3, 0),
        0x1B => q(K::Particles(Particles::Any), Unit::PerM3, 5),
        0x1C => q(K::Particles(Particles::Pm1), Unit::PerM3, 5),
        0x1D => q(K::Particles(Particles::Pm2_5), Unit::PerM3, 5),
        0x1E => q(K::Particles(Particles::Pm10), Unit::PerM3, 5),
        0x1F => q(K::Illuminance, Unit::Lux, 0),
        0x20 => q(K::LuminousIntensity, Unit::Candela, 0),
        0x21 => q(K::Irradiance, Unit::WattPerM2, 0),
        0x22 => q(K::WindSpeed, Unit::MetrePerS, 0),
        0x23 => n(K::Rainfall),
        0x24 => q(K::SoundLevel, Unit::Dba, 0),
        0x25 => q(K::Turbidity, Unit::Fnu, 0),
        0x27 => q(K::Ph, Unit::Ph, -1),
        0x2C => n(K::Events(Event::Dismounts)),
        0x2D => n(K::Events(Event::TestButton)),
        0x2E => n(K::Events(Event::Alarms)),
        0x2F => n(K::Events(Event::AlarmMutes)),
        0x30 => n(K::Events(Event::Obstacles)),
        0x31 => n(K::Events(Event::SmokeEntryBlocked)),
        0x32 => n(K::Events(Event::ChamberDefects)),
        0x33 => n(K::Events(Event::SelfTestFailures)),
        0x34 => n(K::Events(Event::SounderDefects)),
        0x38 => q(K::BatteryStatus, Unit::Percent, 0),
        0x39 => q(K::ChamberPollution, Unit::Percent, 0),
        0x3A | 0x3B => q(K::Distance, Unit::Millimetre, exp(c, 1, 0)),
        0x3E => q(K::Moisture, Unit::Percent, 0),
        0x40 => n(K::StatusBits(Sensor::Pressure)),
        0x41 => n(K::StatusBits(Sensor::Smoke)),
        0x42 => n(K::StatusBits(Sensor::CarbonMonoxide)),
        0x43 => n(K::StatusBits(Sensor::Heat)),
        0x44 => n(K::StatusBits(Sensor::DoorWindow)),
        _ => n(K::Reserved { table: T, code: c }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vib(vif: u8, vifes: &[u8]) -> Vib<'static> {
        let mut v = [0u8; MAX_EXTENSIONS];
        v[..vifes.len()].copy_from_slice(vifes);
        Vib::new(vif, v, u8::try_from(vifes.len()).unwrap(), None)
    }

    #[test]
    fn modifiers_skip_the_table_code_and_stop_at_manufacturer() {
        let v = vib(0xFD, &[0x97, 0x22]);
        assert_eq!(v.description().kind, Kind::ErrorFlags);
        assert_eq!(
            v.modifiers().collect::<heapless_vec::V>().0,
            [Some(Modifier::Per(Per::Hour)), None, None]
        );
        let v = vib(0x93, &[0xFF, 0x75]);
        assert_eq!(v.modifiers().count(), 1);
        let v = vib(0xFF, &[0x75]);
        assert_eq!(v.modifiers().count(), 0);
        let v = vib(0x93, &[0xFC, 0x01]);
        assert_eq!(v.modifiers().next(), Some(Modifier::Extension(Extension::Phase(1))));
        let v = vib(0xFD, &[0xFD, 0x8E, 0x75]);
        assert_eq!(v.description().table, Table::FdSecond);
        assert_eq!(v.modifiers().next(), Some(Modifier::Scale(-1)));
        assert_eq!(vib(0xFD, &[]).description().kind, Kind::MissingExtension(Table::Fd));
    }

    // A three-slot collector so the test stays allocation-free like the crate.
    mod heapless_vec {
        pub struct V(pub [Option<super::Modifier>; 3]);
        impl FromIterator<super::Modifier> for V {
            fn from_iter<I: IntoIterator<Item = super::Modifier>>(it: I) -> Self {
                let mut out = [None; 3];
                for (slot, m) in out.iter_mut().zip(it) {
                    *slot = Some(m);
                }
                Self(out)
            }
        }
    }
}
