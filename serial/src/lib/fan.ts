import { Device } from "./modbus";

enum InputRegisterAddress {
  Identification = 0xd000,
  MaxCountBytes = 0xd001,
  SoftwareNameBusController = 0xd002,
  SoftwareVersionBusController = 0xd003,
  SoftwareNameCommutationController = 0xd004,
  SoftwareVersionCommutationController = 0xd005,
  CurrentRpm = 0xd010,
  MotorStatus = 0xd011,
  Warning = 0xd012,
  /**
   * Also known as DC link voltage
   * German: Zwischenkreisspannung
   */
  VoltageIntermediateCircuit = 0xd013,
  /**
   * Also known as DC link current
   * German: Zwischenkreisstrom
   */
  CurrentIntermediateCircuit = 0xd014,
  MotorTemperature = 0xd016,
  ElectronicInteriorTemperature = 0xd017,
  /**
   * German: Aussteuergrad
   * Note: Not sure on translation as I lack the technical knowledge
   */
  PhaseControlFactor = 0xd019,
  /**
   * Set point?
   * German: Aktueller Sollwert
   */
  CurrentTargetValue = 0xd01a,
  /**
   * German: Sensoristwert
   */
  CurrentSensorValue = 0xd01b,
  /**
   * German: Sensoristwert 1
   */
  CurrentSensorValue1 = 0xd023,

  TemperatureSensor1 = 0xd02e,
  HumiditySensor1 = 0xd02f,
  TemperatureSensor2 = 0xd030,
  HumiditySensor2 = 0xd031,

  /**
   * German: Flügelradanemometer
   */
  RpmVaneAnemometer,

  VolumeFlowAbsolute = 0xd033,
  VolumeFlowRelative = 0xd035,

  /**
   * Mass flow kg/h
   */
  MassFlowAbsolute = 0xd034,
  MassFlowRelative = 0xd036,
  Pt1000TemperatureSensor1 = 0xd038,
  Pt1000TemperatureSensor2 = 0xd039,
  StateEnableInput = 0xd01c,
  /**
   * German: Aktueller Wirksinn
   */
  CurrentDesiredEffect = 0xd01e,
  CurrentPowerRelative = 0xd021,
  /**
   * Absolute in Watt
   */
  CurrentPowerAbsolute = 0xd027,
  CurrentTargetValueSource = 0xd028,
  /**
   * Energt consumption meter LSB (Least significant byte (or bit?))
   */
  EnergyConsumptionMeterLsb = 0xd029,

  /**
   * Energt consumption meter MSB (most significant byte (or bit?))
   */
  EnergyConsumptionMeterMsb = 0xd029,
  Heartbeat = 0xd037,
  MirroredInputRegister1 = 0xd100,
  MirroredInputRegister2 = 0xd101,
  MirroredInputRegister3 = 0xd102,
  MirroredInputRegister4 = 0xd103,
  MirroredInputRegister5 = 0xd104,
  MirroredInputRegister6 = 0xd105,
  MirroredInputRegister7 = 0xd106,
  MirroredInputRegister8 = 0xd107,
  MirroredInputRegister9 = 0xd108,
  MirroredInputRegister10 = 0xd109,
  MirroredInputRegister11 = 0xd10a,
  MirroredInputRegister12 = 0xd10b,
  MirroredInputRegister13 = 0xd10c,
  MirroredInputRegister14 = 0xd10d,
  MirroredInputRegister15 = 0xd10e,
  MirroredInputRegister16 = 0xd1f,
}

enum HoldingRegisterAddress {
  Reset = 0xd000,
  DefaultTargetValue = 0xd001,
  PasswordRegister1 = 0xd002,
  PasswordRegister2 = 0xd003,
  PasswordRegister3 = 0xd004,
  FactorySettingControl = 0xd005,
  CustomerSettingControl = 0xd006,
  OperatingHoursCounter = 0xd009,
  OperatingMinutesCounter = 0xd00a,
  AddressingFunctionOnOff = 0xd00c,
  SavedTargetValue = 0xd00d,
  EnableRs485 = 0xd00f,
  FanAddress = 0xd100,
  TargetValueSource = 0xd101,
  PreferredRunningDirection = 0xd102,
  TargetValueSaving = 0xd103,
  OperatingMode = 0xd106,
  /**
   * German: Wirksinn
   */
  DesiredEffect = 0xd108,
  ControlParameterPFactor = 0xd10a,
  ControlParameterKFactor = 0xd10c,
  /**
   * German: Maximaler Aussteuergrad
   */
  MaximumPhaseControlFactor = 0xd10e,
  /**
   * German:  Minimum Aussteuergrad
   */
  MinimumPhaseControlFactor = 0xd110,
  MotorStopEnable = 0xd112,
  StartPhaseControlFactor = 0xd116,
  MaximumAllowedPhaseControlFactor = 0xd117,
  MinimumAllowedPhaseControlFactor = 0xd118,
  MaximumRpm = 0xd119,
  MaximumAllowedRpm = 0xd11a,
  /**
   * German: Hochlauframpe
   */
  AccelerationRamp = 0xd11f,
  /**
   * German: Auslauframpe
   */
  DeccelarationRamp = 0xd120,
  RpmLimit = 0xd128,
  InputCharacteristicCurvePoint1X = 0xd12a,
  InputCharacteristicCurvePoint1Y = 0xd12b,
  InputCharacteristicCurvePoint2X = 0xd12c,
  InputCharacteristicCurvePoint2Y = 0xd12d,
  MaximumPower = 0xd155,
  MaximumAllowedPower = 0xd135,
  RpmLimitActionMonitoring = 0xd145,
  SensorCurrentValueSource = 0xd147,
  BaudRate = 0xd149,
  Parity = 0xd14a,
  EmergencyRunningDirection = 0xd15b,
  EmergencyTargetValueSource = 0xd15c,
  EmergencyTargetValue = 0xd15d,
  EmergencyDelay = 0xd15e,
  InputCharacteristicLimitCableBreak = 0xd15f,
  MinimumSensorValue1 = 0xd160,
  MinimumSensorValue2 = 0xd161,
  MaximumSensorValue1 = 0xd161,
  MaximumSensorValue2 = 0xd161,
  SensorAddress1 = 0xd165,
  SensorAddress2 = 0xd165,
  SensorAddress3 = 0xd166,
  SensorAddress4 = 0xd167,
  SensorAddress5 = 0xd168,
  SensorAddress6 = 0xd169,
  EnableSource = 0xd16a,
  SavedEnableRs485 = 0xd16b,
  CustomerData1 = 0xd170,
  CustomerData2 = 0xd171,
  CustomerData3 = 0xd172,
  CustomerData4 = 0xd173,
  CustomerData5 = 0xd174,
  CustomerData6 = 0xd175,
  CustomerData7 = 0xd176,
  CustomerData8 = 0xd177,
  CustomerData9 = 0xd178,
  CustomerData10 = 0xd179,
  CustomerData11 = 0xd17a,
  CustomerData12 = 0xd17b,
  CustomerData13 = 0xd17c,
  CustomerData14 = 0xd17d,
  CustomerData15 = 0xd17e,
  CustomerData16 = 0xd17f,

  ErrorPointer = 0xd182,

  Error1 = 0xd184,
  Error1Time = 0xd185,

  ErrorHistory1 = 0xd186,
  ErrorHistory1Time = 0xd187,

  ErrorHistory2 = 0xd188,
  ErrorHistory2Time = 0xd189,

  ErrorHistory3 = 0xd18a,
  ErrorHistory3Time = 0xd18b,

  ErrorHistory4 = 0xd18c,
  ErrorHistory4Time = 0xd18d,

  ErrorHistory5 = 0xd18e,
  ErrorHistory5Time = 0xd18f,

  ErrorHistory6 = 0xd190,
  ErrorHistory6Time = 0xd191,

  ErrorHistory7 = 0xd192,
  ErrorHistory7Time = 0xd193,

  ErrorHistory8 = 0xd194,
  ErrorHistory8Time = 0xd195,

  ErrorHistory9 = 0xd196,
  ErrorHistory9Time = 0xd197,

  ErrorHistory10 = 0xd198,
  ErrorHistory10Time = 0xd199,

  ErrorHistory11 = 0xd19a,
  ErrorHistory11Time = 0xd19b,

  ErrorHistory12 = 0xd19c,
  ErrorHistory12Time = 0xd19d,

  ErrorHistory13 = 0xd19e,
  ErrorHistory13Time = 0xd19f,

  VoltageIntermediateCircuitReferenceValue = 0xd1a0,
  CurrentIntermediateCircuitReferenceValue = 0xd1a1,
  SerialNumber1 = 0xd1a2,
  SerialNumber2 = 0xd1a3,
  ProductionDate = 0xd1a4,
  VolumeFlowReferenceValue = 0xd1ed,
  MassFlowReferenceValue = 0xd1ed,

  /**
   * The original address register stores the register address of the register that should be mirrored.
   * Mirrored registers help sequencially reading registers that might not be next to each other
   */
  MirroredHoldingRegister1OriginalAddress = 0xd480,
  MirroredHoldingRegister2OriginalAddress = 0xd481,
  MirroredHoldingRegister3OriginalAddress = 0xd482,
  MirroredHoldingRegister4OriginalAddress = 0xd483,
  MirroredHoldingRegister5OriginalAddress = 0xd484,
  MirroredHoldingRegister6OriginalAddress = 0xd485,
  MirroredHoldingRegister7OriginalAddress = 0xd486,
  MirroredHoldingRegister8OriginalAddress = 0xd487,
  MirroredHoldingRegister9OriginalAddress = 0xd488,
  MirroredHoldingRegister10OriginalAddress = 0xd489,
  MirroredHoldingRegister11OriginalAddress = 0xd48a,
  MirroredHoldingRegister12OriginalAddress = 0xd48b,
  MirroredHoldingRegister13OriginalAddress = 0xd48c,
  MirroredHoldingRegister14OriginalAddress = 0xd48d,
  MirroredHoldingRegister15OriginalAddress = 0xd48e,
  MirroredHoldingRegister16OriginalAddress = 0xd48f,
  MirroredHoldingRegister17OriginalAddress = 0xd490,
  MirroredHoldingRegister18OriginalAddress = 0xd491,
  MirroredHoldingRegister19OriginalAddress = 0xd492,
  MirroredHoldingRegister20OriginalAddress = 0xd493,
  MirroredHoldingRegister21OriginalAddress = 0xd494,
  MirroredHoldingRegister22OriginalAddress = 0xd495,
  MirroredHoldingRegister23OriginalAddress = 0xd496,
  MirroredHoldingRegister24OriginalAddress = 0xd497,
  MirroredHoldingRegister25OriginalAddress = 0xd498,
  MirroredHoldingRegister26OriginalAddress = 0xd499,
  MirroredHoldingRegister27OriginalAddress = 0xd49a,
  MirroredHoldingRegister28OriginalAddress = 0xd49b,
  MirroredHoldingRegister29OriginalAddress = 0xd49c,
  MirroredHoldingRegister30OriginalAddress = 0xd49d,
  MirroredHoldingRegister31OriginalAddress = 0xd49e,
  MirroredHoldingRegister32OriginalAddress = 0xd49f,

  /**
   * The registers where the values from the orginal address register can be read
   */
  MirroredHoldingRegister1 = 0xd380,
  MirroredHoldingRegister2 = 0xd381,
  MirroredHoldingRegister3 = 0xd382,
  MirroredHoldingRegister4 = 0xd383,
  MirroredHoldingRegister5 = 0xd384,
  MirroredHoldingRegister6 = 0xd385,
  MirroredHoldingRegister7 = 0xd386,
  MirroredHoldingRegister8 = 0xd387,
  MirroredHoldingRegister9 = 0xd388,
  MirroredHoldingRegister10 = 0xd389,
  MirroredHoldingRegister11 = 0xd38a,
  MirroredHoldingRegister12 = 0xd38b,
  MirroredHoldingRegister13 = 0xd38c,
  MirroredHoldingRegister14 = 0xd38d,
  MirroredHoldingRegister15 = 0xd38e,
  MirroredHoldingRegister16 = 0xd38f,
  MirroredHoldingRegister17 = 0xd390,
  MirroredHoldingRegister18 = 0xd391,
  MirroredHoldingRegister19 = 0xd392,
  MirroredHoldingRegister20 = 0xd393,
  MirroredHoldingRegister21 = 0xd394,
  MirroredHoldingRegister22 = 0xd395,
  MirroredHoldingRegister23 = 0xd396,
  MirroredHoldingRegister24 = 0xd397,
  MirroredHoldingRegister25 = 0xd398,
  MirroredHoldingRegister26 = 0xd399,
  MirroredHoldingRegister27 = 0xd39a,
  MirroredHoldingRegister28 = 0xd39b,
  MirroredHoldingRegister29 = 0xd39c,
  MirroredHoldingRegister30 = 0xd39d,
  MirroredHoldingRegister31 = 0xd39e,
  MirroredHoldingRegister32 = 0xd39f,

  MirroredInputRegister1OriginalAddress = 0xd400,
  MirroredInputRegister2OriginalAddress = 0xd401,
  MirroredInputRegister3OriginalAddress = 0xd402,
  MirroredInputRegister4OriginalAddress = 0xd403,
  MirroredInputRegister5OriginalAddress = 0xd404,
  MirroredInputRegister6OriginalAddress = 0xd405,
  MirroredInputRegister7OriginalAddress = 0xd406,
  MirroredInputRegister8OriginalAddress = 0xd407,
  MirroredInputRegister9OriginalAddress = 0xd408,
  MirroredInputRegister10OriginalAddress = 0xd409,
  MirroredInputRegister11OriginalAddress = 0xd40a,
  MirroredInputRegister12OriginalAddress = 0xd40b,
  MirroredInputRegister13OriginalAddress = 0xd40c,
  MirroredInputRegister14OriginalAddress = 0xd40d,
  MirroredInputRegister15OriginalAddress = 0xd40e,
  MirroredInputRegister16OriginalAddress = 0xd40f,

  MassFlowHeightAboveSeaLevel = 0xd602,
}

export class Fan extends Device {
  inputRegisters = {
    heartbeat: this.createInputRegister({
      address: InputRegisterAddress.Heartbeat,
      length: 2,
      deserialize(view) {
        return view.getUint16(0);
      },
    }),
  };
  holdingRegisters = {};
}
