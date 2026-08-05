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
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(chunk);
        let mut lines = Vec::new();

        loop {
            // 找最早的换行符位置
            let (nl_pos, consumed) = match self.find_newline() {
                Some(p) => p,
                None => break,
            };
            // 行内容 = pending[..nl_pos]
            let line: Vec<u8> = self.pending.drain(..nl_pos).collect();
            // 移除换行符本身（drain 掉 consumed 个字节）
            for _ in 0..consumed {
                self.pending.remove(0);
            }
            lines.push(line);
        }
        lines
    }

    /// 在 pending 开头查找换行符，返回 (行内容结束位置, 换行符字节数)。
    /// 优先匹配 \r\n（2字节），再 \n / \r（1字节）。
    ///
    /// 注意：若 \r 为 pending 最后一个字节，不确定是 \r\n 还是 \r，
    /// 延迟到下一次 feed 再判断，避免跨 read 边界切分时把 \r\n 拆成两行。
    fn find_newline(&self) -> Option<(usize, usize)> {
        for i in 0..self.pending.len() {
            let b = self.pending[i];
            if b == b'\r' {
                // \r 是最后一个字节 → 可能只是 \r\n 的前半，等更多数据
                if i + 1 >= self.pending.len() {
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
}
