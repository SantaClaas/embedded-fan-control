use defmt::{info, Format};
use embassy_net::EthernetAddress;

#[derive(Debug, Format)]
enum MessageType {
    Request = 0x01,
    Reply = 0x02,
}

#[derive(Debug, Format)]
enum HardwareAddressType {
    Ethernet = 0x01,
}

#[derive(Debug, Format)]
enum Reply {
    Unicast = 0x01,
    Broadcast = 0x02,
}

#[derive(Debug, Format)]
pub(crate) struct Packet<'a> {
    option: MessageType,
    address_type: HardwareAddressType,
    hardware_address_length: u8,
    hops_count: u8,
    transaction_id: u32,
    seconds_elapsed: u16,
    flags: Reply,
    client_address: embassy_net::Ipv4Address,
    your_address: embassy_net::Ipv4Address,
    gateway_address: embassy_net::Ipv4Address,
    client_hardware_address: EthernetAddress,
    options: Options<'a>,
}

#[derive(Debug, Format)]
enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Acknowledge = 5,
    NotAcknowledge = 6,
    Release = 7,
    Inform = 8,
    ForceRenew = 9,
    LeaseQuery = 10,
    LeaseUnassigned = 11,
    LeaseUnknown = 12,
    LeaseActive = 13,
}

impl TryFrom<u8> for DhcpMessageType {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DhcpMessageType::Discover),
            2 => Ok(DhcpMessageType::Offer),
            3 => Ok(DhcpMessageType::Request),
            4 => Ok(DhcpMessageType::Decline),
            5 => Ok(DhcpMessageType::Acknowledge),
            6 => Ok(DhcpMessageType::NotAcknowledge),
            7 => Ok(DhcpMessageType::Release),
            8 => Ok(DhcpMessageType::Inform),
            9 => Ok(DhcpMessageType::ForceRenew),
            10 => Ok(DhcpMessageType::LeaseQuery),
            11 => Ok(DhcpMessageType::LeaseUnassigned),
            12 => Ok(DhcpMessageType::LeaseUnknown),
            13 => Ok(DhcpMessageType::LeaseActive),
            other => Err(DecodeError::UnknownDhcpMessageType(other)),
        }
    }
}

#[derive(Debug, Format)]
enum MaybeString<'a> {
    String(&'a str),
    Bytes(&'a [u8]),
}

#[derive(Debug, Format, Default)]
struct Options<'a> {
    host_name: Option<MaybeString<'a>>,
    messagge_type: Option<DhcpMessageType>,
    parameter_request_list: Option<&'a [u8]>,
    maximum_dhcp_message_size: Option<u16>,
    vendor_class_identifier: Option<MaybeString<'a>>,
    client_identifier: Option<&'a [u8]>,
    rapid_commit: Option<()>,
    lease_seconds: Option<u32>,
}

#[derive(Debug, Format)]
pub(crate) enum DecodeError {
    UnkownMessageType(u8),
    UnkownHardwareAddressType(u8),
    UnkownFlag(u8),
    /// The hardware address length is not 6
    UnsupportedHardwareAddressLength(u8),
    UnsupportedOption(u8),
    UnknownDhcpMessageType(u8),
    InvalidOptionLength {
        option: u8,
        expected: usize,
        actual: usize,
    },
}

impl<'a> Packet<'a> {
    pub(crate) fn try_decode(buffer: &'a [u8]) -> Result<Self, DecodeError> {
        let option = match buffer[0] {
            0x01 => MessageType::Request,
            0x02 => MessageType::Reply,
            other => return Err(DecodeError::UnkownMessageType(other)),
        };

        let address_type = match buffer[1] {
            0x01 => HardwareAddressType::Ethernet,
            other => return Err(DecodeError::UnkownHardwareAddressType(other)),
        };

        let hardware_address_length = buffer[2];
        if hardware_address_length != 6 {
            return Err(DecodeError::UnsupportedHardwareAddressLength(
                hardware_address_length,
            ));
        }

        let hops_count = buffer[3];

        let transaction_id = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let seconds_elapsed = u16::from_be_bytes([buffer[8], buffer[9]]);
        let flags = match buffer[10] {
            0 => Reply::Unicast,
            0b1000_0000 => Reply::Broadcast,
            other => return Err(DecodeError::UnkownFlag(other)),
        };

        let client_address =
            embassy_net::Ipv4Address::new(buffer[11], buffer[12], buffer[13], buffer[14]);

        // Address assigned by the server
        let your_address =
            embassy_net::Ipv4Address::new(buffer[15], buffer[16], buffer[17], buffer[18]);

        let gateway_address =
            embassy_net::Ipv4Address::new(buffer[19], buffer[20], buffer[21], buffer[22]);

        // The length matches hardware address length earlier which we ensured is 6
        // The 5 bytes padding left and right of the address is apparently a remnant of the BOOTP protocol which DHCP is based on
        let client_hardware_address = &buffer[23..23 + 16];
        // 5 zero bytes + 6 hardware address bytes + 5 zero bytes = 16 bytes
        info!("Client address: {:x}", &client_hardware_address);
        let client_hardware_address = EthernetAddress::from_bytes(&client_hardware_address[5..11]);
        // Then there are 192 octets of 0s apparently
        const MAGIC_COOKIE_START: usize = 23 + 16 + 5 + 192;
        const MAGIC_COOKIE_END: usize = MAGIC_COOKIE_START + 4;
        let magic_cookie = &buffer[MAGIC_COOKIE_START..MAGIC_COOKIE_END];
        info!("Magic cookie: {:x}", &magic_cookie);
        // Variable part with options
        let mut offset = MAGIC_COOKIE_END;

        info!("Reading options");
        let mut options = Options::default();
        while let Some(option) = buffer.get(offset).copied() {
            offset += 1;
            let length = buffer[offset] as usize;
            offset += 1;
            let content = &buffer[offset - 2..offset + length];
            info!(
                "\n  Option: {:?}\n  Length : {:?}\n  Contents: {:x} {:?}",
                option, length, content, content
            );

            match option {
                12 => {
                    // Host name
                    let host_name = &buffer[offset..offset + length];
                    info!("Host name: {:x} {:?}", host_name, host_name);
                    options.host_name = match core::str::from_utf8(host_name) {
                        Ok(host_name) => Some(MaybeString::String(host_name)),
                        Err(_) => {
                            // Host name is not a valid UTF-8 string
                            Some(MaybeString::Bytes(host_name))
                        }
                    };
                }
                51 => {
                    // IP address lease time
                    if length != 4 {
                        return Err(DecodeError::InvalidOptionLength {
                            option,
                            expected: 4,
                            actual: length,
                        });
                    }

                    let lease_seconds = u32::from_be_bytes([
                        buffer[offset],
                        buffer[offset + 1],
                        buffer[offset + 2],
                        buffer[offset + 3],
                    ]);
                    info!("Lease Seconds: {}", lease_seconds);
                    options.lease_seconds = Some(lease_seconds);
                }
                53 => {
                    // Should be 1 byte length for this option
                    let message_type: DhcpMessageType = buffer[offset].try_into()?;
                    info!("DHCP Message Type: {:?}", message_type);
                    options.messagge_type = Some(message_type);
                }
                55 => {
                    // Parameter request list
                    let parameter_request_list = &buffer[offset..offset + length];
                    info!(
                        "Parameter request list: {:x} {:?}",
                        parameter_request_list, parameter_request_list
                    );
                    options.parameter_request_list = Some(parameter_request_list);
                }
                57 => {
                    if length != 2 {
                        return Err(DecodeError::InvalidOptionLength {
                            option,
                            expected: 2,
                            actual: length,
                        });
                    }

                    // Maximum DHCP message size
                    // Has to be 2 bytes
                    let size = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]);
                    info!("Maximum DHCP message size {:x} {:?}", size, size);
                    options.maximum_dhcp_message_size = Some(size);
                }
                60 => {
                    // Vendor class identifier
                    let vendor_class_identifier = &buffer[offset..offset + length];

                    options.vendor_class_identifier =
                        match core::str::from_utf8(vendor_class_identifier) {
                            Ok(vendor_class_identifier) => {
                                Some(MaybeString::String(vendor_class_identifier))
                            }
                            Err(_) => {
                                // Vendor class identifier is not a valid UTF-8 string
                                Some(MaybeString::Bytes(vendor_class_identifier))
                            }
                        };

                    info!(
                        "Vendor class identifier: {:?}",
                        options.vendor_class_identifier
                    );
                }
                61 => {
                    // Client identifier
                    let client_identifier = &buffer[offset..offset + length];
                    info!(
                        "Client identifier: {:x} {:?}",
                        client_identifier, client_identifier
                    );
                    options.client_identifier = Some(client_identifier);
                }
                80 => {
                    // Rapid commit
                    info!("Rapid commit");
                    options.rapid_commit = Some(());
                }
                255 => {
                    // This marks the end
                    break;
                }

                other => return Err(DecodeError::UnsupportedOption(other)),
            }

            offset += length;
        }

        Ok(Self {
            option,
            address_type,
            hardware_address_length,
            hops_count,
            transaction_id,
            seconds_elapsed,
            flags,
            client_address,
            your_address,
            gateway_address,
            client_hardware_address,
            options,
        })
    }
}
