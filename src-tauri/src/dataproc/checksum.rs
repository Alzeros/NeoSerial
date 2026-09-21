//! 帧校验算法。CRC 用 `crc` crate 的目录常量,SUM8/XOR8 手写。
//! 本模块只产出**大端规范序**字节(`to_be_bytes`);端序(LE/BE)由调用方拼帧时翻转。
//! SUM8/XOR8 是单字节,不受端序影响。

use serde::{Deserialize, Serialize};

/// 校验算法预设。`None` 由调用方处理(不调用本模块)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumAlgo {
    None,
    Crc16Modbus,
    Crc16CcittFalse,
    Crc16Xmodem,
    Crc16Arc,
    Crc32,
    Sum8,
    Xor8,
}

/// 计算 data 的校验字节(大端规范序)。`None` 返回空 Vec。
pub fn compute_checksum(algo: ChecksumAlgo, data: &[u8]) -> Vec<u8> {
    match algo {
        ChecksumAlgo::None => Vec::new(),
        ChecksumAlgo::Crc16Modbus => crc::Crc::<u16>::new(&crc::CRC_16_MODBUS).checksum(data).to_be_bytes().to_vec(),
        ChecksumAlgo::Crc16CcittFalse => crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(data).to_be_bytes().to_vec(),
        ChecksumAlgo::Crc16Xmodem => crc::Crc::<u16>::new(&crc::CRC_16_XMODEM).checksum(data).to_be_bytes().to_vec(),
        ChecksumAlgo::Crc16Arc => crc::Crc::<u16>::new(&crc::CRC_16_ARC).checksum(data).to_be_bytes().to_vec(),
        ChecksumAlgo::Crc32 => crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(data).to_be_bytes().to_vec(),
        ChecksumAlgo::Sum8 => vec![data.iter().fold(0u8, |a, &b| a.wrapping_add(b))],
        ChecksumAlgo::Xor8 => vec![data.iter().fold(0u8, |a, &b| a ^ b)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const V: &[u8] = b"123456789";

    #[test]
    fn crc_vectors() {
        // crc-catalog check 值
        assert_eq!(compute_checksum(ChecksumAlgo::Crc16Modbus, V), vec![0x4B, 0x37]);
        assert_eq!(compute_checksum(ChecksumAlgo::Crc16CcittFalse, V), vec![0x29, 0xB1]);
        assert_eq!(compute_checksum(ChecksumAlgo::Crc16Xmodem, V), vec![0x31, 0xC3]);
        assert_eq!(compute_checksum(ChecksumAlgo::Crc16Arc, V), vec![0xBB, 0x3D]);
        assert_eq!(compute_checksum(ChecksumAlgo::Crc32, V), vec![0xCB, 0xF4, 0x39, 0x26]);
    }

    #[test]
    fn sum8_xor8() {
        assert_eq!(compute_checksum(ChecksumAlgo::Sum8, &[0x01, 0x02, 0xFF]), vec![0x02]); // 0x102 -> 0x02
        assert_eq!(compute_checksum(ChecksumAlgo::Xor8, &[0x01, 0x02, 0xFF]), vec![0xFC]); // 01^02=03 ^FF=FC
    }

    #[test]
    fn none_is_empty() {
        assert!(compute_checksum(ChecksumAlgo::None, V).is_empty());
    }
}
