//! 帧构造纯函数。固定四段式:[帧头][长度][数据][校验]。
//! 无状态、可注入 rng,便于单测;command 层只做薄封装。

use serde::{Deserialize, Serialize};
use super::checksum::{compute_checksum, ChecksumAlgo};

/// 单帧总长上限:发送是整帧单次 write + 回显整帧序列化,1MiB 会堵写线程/刷爆 webview。
pub const MAX_FRAME_LEN: usize = 64 * 1024; // 64 KiB

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Endian { Le, Be }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthSize { None, One, Two }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthCoverage { Data, DataPlusChecksum, WholeFrame }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormat { Hex, Text }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillPattern { Zeros, Ff, RandomBytes, RandomText, Increment, CustomLoop }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthBasis { DataField, WholeFrame }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LengthFieldSpec {
    pub size: LengthSize,
    pub endian: Endian,
    pub coverage: LengthCoverage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSpec {
    pub mode: DataMode,
    pub format: ContentFormat,
    pub value: String,
    pub pattern: FillPattern,
    pub charset: String,
    pub start: u8,
    pub custom_format: ContentFormat,
    pub custom_value: String,
    pub length_basis: LengthBasis,
    pub length: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataMode { Content, Fill }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChecksumSpec {
    pub algo: ChecksumAlgo,
    pub endian: Endian,
    pub include_header: bool,
    pub include_length: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameConfig {
    pub header_hex: String,
    pub length: LengthFieldSpec,
    pub data: DataSpec,
    pub checksum: ChecksumSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind { Header, Length, Data, Checksum }

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub offset: usize,
    pub len: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrameResult {
    pub hex: String,
    pub total_len: usize,
    pub breakdown: Vec<Segment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildField { Header, Length, Data, Custom, Charset }

#[derive(Clone, Debug, Serialize)]
pub struct FrameBuildError {
    pub field: Option<BuildField>,
    pub message: String,
}

impl FrameBuildError {
    fn new(field: BuildField, message: impl Into<String>) -> Self {
        Self { field: Some(field), message: message.into() }
    }
}

/// 构帧。`rng` 仅供 random_bytes / random_text / 每次重构用。
pub fn build_frame(cfg: &FrameConfig, rng: &mut impl rand::Rng) -> Result<FrameResult, FrameBuildError> {
    // 1. 帧头
    let header = parse_hex_field(&cfg.header_hex, BuildField::Header)?;

    // 2. 数据域(内容 或 填充;填充长度按 basis 推导,上限在 build_data_field 里前置校验,
    //    不带大 n 进 fill_bytes 分配)
    let data = build_data_field(cfg, rng, header.len())?;

    // 3. 长度字段字节(需要先知道校验长度才能算 data_plus_checksum/whole_frame)
    let checksum_len = checksum_byte_len(cfg.checksum.algo);
    let data_plus_cs = data.len() + checksum_len;
    let len_field_size = length_field_size(cfg.length.size);
    let length_value = match cfg.length.coverage {
        LengthCoverage::Data => data.len(),
        LengthCoverage::DataPlusChecksum => data_plus_cs,
        LengthCoverage::WholeFrame => header.len() + len_field_size + data.len() + checksum_len,
    };
    let length_bytes = encode_length(cfg.length.size, cfg.length.endian, length_value)?;

    // 4. 校验输入 = [头?][长度?][数据];校验值大端规范序,LE 且多字节时 reverse
    let mut cs_input = Vec::new();
    if cfg.checksum.include_header { cs_input.extend_from_slice(&header); }
    if cfg.checksum.include_length { cs_input.extend_from_slice(&length_bytes); }
    cs_input.extend_from_slice(&data);
    let mut checksum = compute_checksum(cfg.checksum.algo, &cs_input);
    if checksum.len() > 1 && cfg.checksum.endian == Endian::Le { checksum.reverse(); }

    // 5. 拼帧 + 上限
    let mut frame = Vec::new();
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&length_bytes);
    frame.extend_from_slice(&data);
    frame.extend_from_slice(&checksum);
    if frame.is_empty() {
        return Err(FrameBuildError::new(BuildField::Data, "帧为空"));
    }
    if frame.len() > MAX_FRAME_LEN {
        return Err(FrameBuildError::new(BuildField::Length, format!("单帧超过 64 KiB 上限(当前 {} 字节)", frame.len())));
    }

    // 6. breakdown(规范 hex 串第 i 字节占 [3i,3i+2))
    let mut breakdown = Vec::new();
    let mut off = 0;
    for (kind, seg) in [(SegmentKind::Header, &header), (SegmentKind::Length, &length_bytes), (SegmentKind::Data, &data), (SegmentKind::Checksum, &checksum)] {
        if !seg.is_empty() { breakdown.push(Segment { kind, offset: off, len: seg.len() }); }
        off += seg.len();
    }

    Ok(FrameResult { hex: bytes_to_spaced_hex(&frame), total_len: frame.len(), breakdown })
}

fn parse_hex_field(s: &str, field: BuildField) -> Result<Vec<u8>, FrameBuildError> {
    crate::util::codec::hex_to_bytes(s).map_err(|e| FrameBuildError::new(field, e))
}

/// 校验段字节数。长度字段按 `data_plus_checksum` / `whole_frame` 口径计数时要算进去。
/// `None` 必须是 0:spec 定"校验为 none 时 `data_plus_checksum` 等价 `data`"。
/// **SUM8/XOR8 是 1 字节,不是 0** —— 写成 0 会让这三种口径各差 1 字节(Task 2 Step 1 的
/// `length_data_plus_checksum_counts_crc` / `length_whole_frame_counts_crc` 专锁这两个分支)。
fn checksum_byte_len(algo: ChecksumAlgo) -> usize {
    match algo {
        ChecksumAlgo::None => 0,
        ChecksumAlgo::Sum8 | ChecksumAlgo::Xor8 => 1,
        ChecksumAlgo::Crc16Modbus | ChecksumAlgo::Crc16CcittFalse | ChecksumAlgo::Crc16Xmodem | ChecksumAlgo::Crc16Arc => 2,
        ChecksumAlgo::Crc32 => 4,
    }
}

/// 长度字段字节数。build_frame 算 whole_frame 口径、build_data_field 推导填充长度共用。
fn length_field_size(size: LengthSize) -> usize {
    match size { LengthSize::None => 0, LengthSize::One => 1, LengthSize::Two => 2 }
}

fn encode_length(size: LengthSize, endian: Endian, value: usize) -> Result<Vec<u8>, FrameBuildError> {
    match size {
        LengthSize::None => Ok(Vec::new()),
        LengthSize::One => {
            if value > 0xFF { return Err(FrameBuildError::new(BuildField::Length, format!("超出 1 字节长度上限 255(当前 {}),请改用 2 字节", value))); }
            Ok(vec![value as u8])
        }
        LengthSize::Two => {
            if value > 0xFFFF { return Err(FrameBuildError::new(BuildField::Length, format!("超出 2 字节长度上限 65535(当前 {})", value))); }
            let b = (value as u16).to_be_bytes();
            Ok(if endian == Endian::Le { vec![b[1], b[0]] } else { b.to_vec() })
        }
    }
}

fn bytes_to_spaced_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

fn build_data_field(cfg: &FrameConfig, rng: &mut impl rand::Rng, header_len: usize) -> Result<Vec<u8>, FrameBuildError> {
    match cfg.data.mode {
        DataMode::Content => match cfg.data.format {
            ContentFormat::Hex => parse_hex_field(&cfg.data.value, BuildField::Data),
            ContentFormat::Text => Ok(cfg.data.value.as_bytes().to_vec()),
        },
        DataMode::Fill => {
            let checksum_len = checksum_byte_len(cfg.checksum.algo);
            let len_field_size = length_field_size(cfg.length.size);
            let n = match cfg.data.length_basis {
                LengthBasis::DataField => cfg.data.length,
                LengthBasis::WholeFrame => {
                    let fixed = header_len + len_field_size + checksum_len;
                    if cfg.data.length < fixed {
                        return Err(FrameBuildError::new(BuildField::Length, format!("整帧长度 {} 不足最小值 {}(头{}+长度{}+校验{})", cfg.data.length, fixed, header_len, len_field_size, checksum_len)));
                    }
                    cfg.data.length - fixed
                }
            };
            // 填充长度上限必须在 fill_bytes 分配之前校验:n 来自用户输入(1e11 量级也能进到这),
            // vec![0u8; n] 分配失败是进程级 abort(不可捕获),而配置已被自动保存,会变成启动即崩循环。
            if n > MAX_FRAME_LEN {
                return Err(FrameBuildError::new(BuildField::Length, format!("填充长度 {} 超出单帧 64 KiB 上限", n)));
            }
            fill_bytes(cfg, rng, n)
        }
    }
}

fn fill_bytes(cfg: &FrameConfig, rng: &mut impl rand::Rng, n: usize) -> Result<Vec<u8>, FrameBuildError> {
    use rand::Rng;
    match cfg.data.pattern {
        FillPattern::Zeros => Ok(vec![0u8; n]),
        FillPattern::Ff => Ok(vec![0xFFu8; n]),
        FillPattern::RandomBytes => Ok((0..n).map(|_| rng.random::<u8>()).collect()),
        FillPattern::RandomText => {
            let cs: Vec<u8> = cfg.data.charset.bytes().collect();
            if cs.is_empty() { return Err(FrameBuildError::new(BuildField::Charset, "随机字符集为空")); }
            if !cfg.data.charset.is_ascii() { return Err(FrameBuildError::new(BuildField::Charset, "随机字符集仅限 ASCII 字符")); }
            Ok((0..n).map(|_| cs[rng.random_range(0..cs.len())]).collect())
        }
        FillPattern::Increment => Ok((0..n).map(|i| cfg.data.start.wrapping_add(i as u8)).collect()),
        FillPattern::CustomLoop => {
            let unit = match cfg.data.custom_format {
                ContentFormat::Hex => parse_hex_field(&cfg.data.custom_value, BuildField::Custom)?,
                ContentFormat::Text => cfg.data.custom_value.as_bytes().to_vec(),
            };
            if unit.is_empty() { return Err(FrameBuildError::new(BuildField::Custom, "自定义循环内容为空")); }
            Ok((0..n).map(|i| unit[i % unit.len()]).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    fn rng() -> StdRng { StdRng::seed_from_u64(42) }

    fn base_cfg() -> FrameConfig {
        FrameConfig {
            header_hex: String::new(),
            length: LengthFieldSpec { size: LengthSize::None, endian: Endian::Le, coverage: LengthCoverage::Data },
            data: DataSpec {
                mode: DataMode::Content, format: ContentFormat::Hex, value: "61 62".into(),
                pattern: FillPattern::Zeros, charset: "A-Za-z0-9".into(), start: 0,
                custom_format: ContentFormat::Hex, custom_value: String::new(),
                length_basis: LengthBasis::DataField, length: 0,
            },
            checksum: ChecksumSpec { algo: ChecksumAlgo::None, endian: Endian::Le, include_header: false, include_length: false },
        }
    }

    #[test]
    fn content_hex_only() {
        // 只有数据域内容 hex 61 62
        let r = build_frame(&base_cfg(), &mut rng()).unwrap();
        assert_eq!(r.hex, "61 62");
        assert_eq!(r.total_len, 2);
    }

    #[test]
    fn header_plus_length_data_coverage() {
        // 头 AA, 长度=数据域字节数(2), 1字节
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.length.coverage = LengthCoverage::Data;
        let r = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(r.hex, "AA 02 61 62");
    }

    #[test]
    fn length_overflow_one_byte() {
        // 300 字节数据域 + 1 字节长度字段 → 报错
        let mut c = base_cfg();
        c.length.size = LengthSize::One;
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Zeros;
        c.data.length = 300;
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("255"));
    }

    #[test]
    fn sum8_over_data() {
        // 校验 SUM8 只覆盖数据域 61 62 -> 0xC3
        let mut c = base_cfg();
        c.checksum.algo = ChecksumAlgo::Sum8;
        let r = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(r.hex, "61 62 C3");
    }

    #[test]
    fn increment_wrap() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Increment;
        c.data.start = 0xFE;
        c.data.length = 4;
        let r = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(r.hex, "FE FF 00 01");
    }

    #[test]
    fn invalid_hex_header() {
        let mut c = base_cfg();
        c.header_hex = "AA5".into(); // 奇数位
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Header));
    }

    // ---- 以下按 spec「测试」一节补全:长度字段矩阵 / 校验覆盖 / 边界 / 填充分支 ----

    /// 长度字段:size × endian(coverage=Data,数据域 2 字节)
    #[test]
    fn length_field_matrix() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 02 61 62"); // one 忽略端序
        c.length.size = LengthSize::Two;
        c.length.endian = Endian::Be;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 00 02 61 62");
        c.length.endian = Endian::Le;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 02 00 61 62");
    }

    /// coverage=data_plus_checksum:校验为 none 时等价 data(spec 明示)
    #[test]
    fn length_data_plus_checksum_none_equals_data() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.length.coverage = LengthCoverage::DataPlusChecksum;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 02 61 62");
    }

    /// coverage=data_plus_checksum + SUM8(1 字节)→ 长度字段必须是 3,不是 2
    #[test]
    fn length_data_plus_checksum_counts_crc() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.length.coverage = LengthCoverage::DataPlusChecksum;
        c.checksum.algo = ChecksumAlgo::Sum8;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 03 61 62 C3");
    }

    /// coverage=whole_frame + SUM8 → 头1+长度1+数据2+校验1 = 5
    #[test]
    fn length_whole_frame_counts_crc() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.length.coverage = LengthCoverage::WholeFrame;
        c.checksum.algo = ChecksumAlgo::Sum8;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 05 61 62 C3");
    }

    /// 校验覆盖 头+长度:CRC16/MODBUS over `AA 02 61 62` = 0x4528(BE)/ 0x2845(LE)
    #[test]
    fn checksum_covers_header_and_length() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.checksum.algo = ChecksumAlgo::Crc16Modbus;
        c.checksum.endian = Endian::Be;
        c.checksum.include_header = true;
        c.checksum.include_length = true;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 02 61 62 45 28");
        c.checksum.endian = Endian::Le; // 多字节校验值按端序翻转
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 02 61 62 28 45");
    }

    /// 四段齐全时 breakdown 的 offset/len 与 hex 串对得上(规范格式第 i 字节占 [3i, 3i+2))
    #[test]
    fn breakdown_aligns_with_hex() {
        let mut c = base_cfg();
        c.header_hex = "AA BB".into();
        c.length.size = LengthSize::One;
        c.checksum.algo = ChecksumAlgo::Crc16Modbus;
        let r = build_frame(&c, &mut rng()).unwrap();
        let kinds: Vec<(SegmentKind, usize, usize)> =
            r.breakdown.iter().map(|s| (s.kind, s.offset, s.len)).collect();
        assert_eq!(kinds, vec![
            (SegmentKind::Header, 0, 2),
            (SegmentKind::Length, 2, 1),
            (SegmentKind::Data, 3, 2),
            (SegmentKind::Checksum, 5, 2),
        ]);
        assert_eq!(r.total_len, 7);
        // 每个 segment 的 offset/len 切出来的 hex 片段长度 = len*3-1
        for s in &r.breakdown {
            let a = 3 * s.offset;
            let b = 3 * (s.offset + s.len) - 1;
            assert_eq!(r.hex[a..b].split(' ').count(), s.len);
        }
    }

    /// 零长数据域 + 帧头/长度可构(心跳帧 `AA 00`);四段合计 0 字节报错
    #[test]
    fn zero_len_data_ok_but_all_empty_errs() {
        let mut c = base_cfg();
        c.data.value = String::new();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA 00");
        c.header_hex = String::new();
        c.length.size = LengthSize::None;
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Data));
        assert!(e.message.contains("帧为空"));
    }

    /// 2 字节长度字段溢出
    #[test]
    fn length_overflow_two_bytes() {
        let mut c = base_cfg();
        c.length.size = LengthSize::Two;
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Zeros;
        c.data.length = 0x10000;
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("65535"));
    }

    /// 64 KiB 上限:正好 65536 通过,+1 字节拒绝
    #[test]
    fn frame_len_limit_64kib() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Zeros;
        c.data.length = 65536;
        assert!(build_frame(&c, &mut rng()).is_ok());
        c.header_hex = "AA".into();
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("64 KiB"));
    }

    /// 填充 length_basis=whole_frame:数据域 = M - 头 - 长度字段 - 校验;M 不足报错
    #[test]
    fn fill_whole_frame_basis() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Increment;
        c.data.length_basis = LengthBasis::WholeFrame;
        c.data.length = 6; // 6 - 头1 - 长度1 - 校验0 = 4
        let r = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(r.hex, "AA 04 00 01 02 03");
        assert_eq!(r.total_len, 6);
        c.data.length = 1; // 不足最小值 2
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("不足"));
    }

    /// custom_loop 非整除回绕(hex 与 text 两种输入)
    #[test]
    fn custom_loop_wraps() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::CustomLoop;
        c.data.custom_format = ContentFormat::Hex;
        c.data.custom_value = "AA BB CC".into();
        c.data.length = 5;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "AA BB CC AA BB");
        c.data.custom_format = ContentFormat::Text;
        c.data.custom_value = "ab".into();
        c.data.length = 5;
        assert_eq!(build_frame(&c, &mut rng()).unwrap().hex, "61 62 61 62 61");
    }

    /// 随机模式:长度正确 + 同种子可复现
    #[test]
    fn random_fill_length_and_determinism() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::RandomBytes;
        c.data.length = 8;
        let a = build_frame(&c, &mut rng()).unwrap();
        let b = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(a.total_len, 8);
        assert_eq!(a.hex, b.hex); // 同种子同结果
        c.data.pattern = FillPattern::RandomText;
        c.data.charset = "AB".into();
        let t = build_frame(&c, &mut rng()).unwrap();
        assert_eq!(t.total_len, 8);
        assert!(t.hex.split(' ').all(|h| h == "41" || h == "42"));
    }

    /// 随机字符集为空 / 含非 ASCII 报错
    #[test]
    fn random_text_bad_charset() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::RandomText;
        c.data.length = 4;
        c.data.charset = String::new();
        assert_eq!(build_frame(&c, &mut rng()).unwrap_err().field, Some(BuildField::Charset));
        c.data.charset = "中".into();
        assert_eq!(build_frame(&c, &mut rng()).unwrap_err().field, Some(BuildField::Charset));
    }

    /// 自定义循环为空 / 数据域 hex 非法
    #[test]
    fn custom_and_data_hex_errors() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::CustomLoop;
        c.data.custom_value = String::new();
        c.data.length = 4;
        assert_eq!(build_frame(&c, &mut rng()).unwrap_err().field, Some(BuildField::Custom));
        let mut d = base_cfg();
        d.data.value = "6".into(); // 奇数位
        assert_eq!(build_frame(&d, &mut rng()).unwrap_err().field, Some(BuildField::Data));
    }

    /// 填充长度超 64 KiB 必须在 fill_bytes 分配之前拒绝(data_field 口径):
    /// 1e11 量级的 vec![0u8; n] 分配失败是进程 abort,不可捕获,只能前置校验挡住。
    #[test]
    fn fill_data_field_huge_length_rejected() {
        let mut c = base_cfg();
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Zeros;
        c.data.length = 100_000_000;
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("64 KiB"));
    }

    /// 同上,whole_frame 口径:总长 1e8 折算出的填充长度同样前置拒绝
    #[test]
    fn fill_whole_frame_huge_length_rejected() {
        let mut c = base_cfg();
        c.header_hex = "AA".into();
        c.length.size = LengthSize::One;
        c.data.mode = DataMode::Fill;
        c.data.pattern = FillPattern::Zeros;
        c.data.length_basis = LengthBasis::WholeFrame;
        c.data.length = 100_000_000;
        let e = build_frame(&c, &mut rng()).unwrap_err();
        assert_eq!(e.field, Some(BuildField::Length));
        assert!(e.message.contains("64 KiB"));
    }
}
