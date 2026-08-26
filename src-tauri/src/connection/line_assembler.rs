/// 跨 read 拼行的状态机。
/// 串口一次 read 的字节边界与换行符边界无关，本结构负责缓存半行，
/// 遇 \r\n / \n / \r 切分出完整行（行内容不含换行符）。
pub struct LineAssembler {
    pending: Vec<u8>,
}

impl LineAssembler {
    pub fn new() -> Self {
        LineAssembler { pending: Vec::new() }
    }

    /// 喂入一段字节，返回本次切出的完整行列表（每行不含换行符）。
    /// 未遇换行符的尾段缓存在内部，等下次 feed。
    ///
    /// 实现：游标扫描——start 之前是已切出的行(含换行符)，扫描/提取都在
    /// [start..] 上进行，循环结束一次性 drain 前缀。旧实现每切一行就
    /// drain(..nl) + 逐字节 remove(0)，每次都是 O(remaining) 的 memmove，
    /// 多行大 chunk 场景退化为 O(n²)；游标法全程 O(n)。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(chunk);
        let mut lines = Vec::new();
        let mut start = 0usize;

        while let Some((nl_pos, consumed)) = self.find_newline_from(start) {
            lines.push(self.pending[start..nl_pos].to_vec());
            start = nl_pos + consumed;
        }
        if start > 0 {
            self.pending.drain(..start);
        }
        lines
    }

    /// 在 pending[start..] 中查找最早的换行符，返回 (行内容结束位置, 换行符字节数)。
    /// 优先匹配 \r\n（2字节），再 \n / \r（1字节）。
    ///
    /// 注意：若 \r 为扫描范围内最后一个字节，不确定是 \r\n 还是 \r，
    /// 返回 None 延迟到下一次 feed 再判断，避免跨 read 边界切分时把 \r\n 拆成两行。
    fn find_newline_from(&self, start: usize) -> Option<(usize, usize)> {
        let len = self.pending.len();
        let mut i = start;
        while i < len {
            let b = self.pending[i];
            if b == b'\r' {
                // \r 是最后一个字节 → 可能只是 \r\n 的前半，等更多数据
                if i + 1 >= len {
                    return None;
                }
                if self.pending[i + 1] == b'\n' {
                    return Some((i, 2));
                }
                return Some((i, 1));
            }
            if b == b'\n' {
                return Some((i, 1));
            }
            i += 1;
        }
        None
    }

    /// 取出残留的半行（无换行符结尾的尾段）。若无可返回 None。
    /// 取出前会去掉尾随的 \r / \n（这些是 deferred 的换行符残留）。
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            return None;
        }
        // 去掉尾随的换行符：\r 可能因 find_newline 的延迟判断而留在 pending 末尾
        while let Some(&b) = self.pending.last() {
            if b == b'\r' || b == b'\n' {
                self.pending.pop();
            } else {
                break;
            }
        }
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_line_crlf() {
        let mut a = LineAssembler::new();
        let lines = a.feed(b"AT\r\n");
        assert_eq!(lines, vec![b"AT".to_vec()]);
    }

    #[test]
    fn test_split_across_reads() {
        // 第一次 feed 末尾的 \r 是 pending 最后一个字节 → 延迟判断，不立即切行。
        let mut a = LineAssembler::new();
        let l1 = a.feed(b"AT+\r");
        assert!(l1.is_empty());
        // 第二次 feed 中 \r 与后续字节相连，逐步切出全部行。
        // 此时 pending = "AT+\rCFUN=1,1\r\nOK\r\n"
        // → \r(i=3) 后跟 'C' → CR 换行 → "AT+"
        // → \r(i=9) 后跟 \n  → \r\n 换行 → "CFUN=1,1"
        // → \r(i=2) 后跟 \n  → \r\n 换行 → "OK"
        let l2 = a.feed(b"CFUN=1,1\r\nOK\r\n");
        assert_eq!(l2, vec![b"AT+".to_vec(), b"CFUN=1,1".to_vec(), b"OK".to_vec()]);
    }

    #[test]
    fn test_lf_only() {
        let mut a = LineAssembler::new();
        let lines = a.feed(b"line1\nline2\n");
        assert_eq!(lines, vec![b"line1".to_vec(), b"line2".to_vec()]);
    }

    #[test]
    fn test_cr_only() {
        // 末尾的 \r 被延迟判断（因不确定是 \r 还是 \r\n 的前半），
        // 需要 flush 才能取出最后一行。
        let mut a = LineAssembler::new();
        let l1 = a.feed(b"abc\rdef\r");
        assert_eq!(l1, vec![b"abc".to_vec()]);
        assert_eq!(a.flush(), Some(b"def".to_vec()));
    }

    #[test]
    fn test_crlf_priority_over_cr() {
        // \r\n 应被当作一个换行符，而非 \r 后接 \n 两次
        let mut a = LineAssembler::new();
        let lines = a.feed(b"a\r\nb\r\n");
        assert_eq!(lines, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_flush_pending() {
        let mut a = LineAssembler::new();
        a.feed(b"incomplete");
        assert_eq!(a.flush(), Some(b"incomplete".to_vec()));
        assert_eq!(a.flush(), None);
    }

    #[test]
    fn test_flush_shell_prompt_tail() {
        // 模拟设备末尾的 "> " 提示符（不换行）：feed 出完整行后，残尾应能通过 flush 取出。
        let mut a = LineAssembler::new();
        let lines = a.feed(b"help\r\n * help\r\n> ");
        assert_eq!(lines, vec![b"help".to_vec(), b" * help".to_vec()]);
        assert_eq!(a.flush(), Some(b"> ".to_vec()));
    }

    #[test]
    fn test_no_newline_returns_empty() {
        let mut a = LineAssembler::new();
        assert!(a.feed(b"no newline here").is_empty());
    }

    #[test]
    fn test_empty_chunk() {
        let mut a = LineAssembler::new();
        assert!(a.feed(b"").is_empty());
    }

    // ===== 重构回归对拍 =====

    /// 确定性 LCG 伪随机字节生成。
    fn lcg(state: &mut u64) -> u8 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*state >> 33) as u8
    }

    /// 参考实现 = 旧算法（逐行 drain + remove(0)），语义与游标版等价。
    /// 本对拍锁住"重构不改变行为"：随机字节流按随机边界切分喂入，
    /// 两种实现的输出行、以及结束后的 flush 残尾必须完全一致。
    fn reference_feed(pending: &mut Vec<u8>, chunk: &[u8]) -> Vec<Vec<u8>> {
        pending.extend_from_slice(chunk);
        let mut lines = Vec::new();
        loop {
            let found = (0..pending.len()).find_map(|i| {
                let b = pending[i];
                if b == b'\r' {
                    if i + 1 >= pending.len() {
                        return None; // \r 在末尾 → 延迟判断（与旧实现一致）
                    }
                    if pending[i + 1] == b'\n' {
                        Some((i, 2))
                    } else {
                        Some((i, 1))
                    }
                } else if b == b'\n' {
                    Some((i, 1))
                } else {
                    None
                }
            });
            let (nl, consumed) = match found {
                Some(p) => p,
                None => break,
            };
            let line: Vec<u8> = pending.drain(..nl).collect();
            for _ in 0..consumed {
                pending.remove(0);
            }
            lines.push(line);
        }
        lines
    }

    #[test]
    fn test_random_stream_matches_reference() {
        let mut seed: u64 = 0x1234_5678_9abc_def0;
        // 提高换行符密度的随机流，接近真实串口数据
        let stream: Vec<u8> = (0..50_000)
            .map(|_| {
                let r = lcg(&mut seed);
                match r % 16 {
                    0 => b'\r',
                    1 => b'\n',
                    _ => r,
                }
            })
            .collect();

        let mut new_asm = LineAssembler::new();
        let mut new_lines: Vec<Vec<u8>> = Vec::new();
        let mut ref_pending: Vec<u8> = Vec::new();
        let mut ref_lines: Vec<Vec<u8>> = Vec::new();

        let mut pos = 0usize;
        while pos < stream.len() {
            let chunk_len = (lcg(&mut seed) % 97) as usize + 1;
            let end = (pos + chunk_len).min(stream.len());
            let chunk = &stream[pos..end];
            pos = end;
            new_lines.extend(new_asm.feed(chunk));
            ref_lines.extend(reference_feed(&mut ref_pending, chunk));
        }

        // 结束后 flush 残尾，两种实现的 pending 状态也必须一致
        let new_tail = new_asm.flush();
        let ref_tail = if ref_pending.is_empty() {
            None
        } else {
            while matches!(ref_pending.last(), Some(b'\r') | Some(b'\n')) {
                ref_pending.pop();
            }
            if ref_pending.is_empty() {
                None
            } else {
                Some(std::mem::take(&mut ref_pending))
            }
        };

        // 测试有效性：足够多的行与残尾场景
        assert!(new_lines.len() > 100, "随机流应切出足量行,实际 {}", new_lines.len());
        assert_eq!(new_lines, ref_lines, "切出行必须与参考实现逐行一致");
        assert_eq!(new_tail, ref_tail, "flush 残尾必须与参考实现一致");
    }
}
