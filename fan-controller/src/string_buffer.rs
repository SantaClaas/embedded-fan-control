use crate::fan;

/// Like a string with length and capacity of 5. Used for sending publish packets to Home Assistant through MQTT
pub(crate) struct StringBuffer<const N: usize> {
    buffer: [u8; N],
    start_index: usize,
}

impl<const N: usize> StringBuffer<N> {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buffer[self.start_index..]
    }
}

impl From<fan::Setting> for StringBuffer<5> {
    fn from(setting: fan::Setting) -> Self {
        // The largest value the set point can assume is 64000 which is 5 characters long
        let mut buffer = [0; 5];
        let mut index = 4;
        let mut remainder = setting.0;
        loop {
            let digit = remainder % 10;
            // Convert digit to ASCII (which is also valid utf-8)
            buffer[index] = digit as u8 + b'0';
            remainder /= 10;
            if remainder <= 0 {
                break;
            }
            index -= 1;
        }

        Self {
            buffer,
            start_index: index,
        }
    }
}
