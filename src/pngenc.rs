//! PNG encoder with real DEFLATE compression (LZ77 hash chains + fixed Huffman). Pure std.
use crate::gfx::Canvas;

struct BitW {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}
impl BitW {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// Huffman codes are sent MSB-first: reverse before packing LSB-first.
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0u32;
        for i in 0..len {
            if code & (1 << i) != 0 {
                r |= 1 << (len - 1 - i);
            }
        }
        self.put(r, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn lit(w: &mut BitW, s: u32) {
    match s {
        0..=143 => w.code(0x30 + s, 8),
        144..=255 => w.code(0x190 + (s - 144), 9),
        256..=279 => w.code(s - 256, 7),
        _ => w.code(0xC0 + (s - 280), 8),
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = BitW { out: Vec::with_capacity(data.len() / 2 + 64), acc: 0, n: 0 };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // fixed Huffman
    const HB: usize = 15;
    let mut head = vec![-1i32; 1 << HB];
    let mut prev = vec![-1i32; data.len().max(1)];
    let hash = |i: usize| -> usize { (((data[i] as u32) << 10 ^ (data[i + 1] as u32) << 5 ^ data[i + 2] as u32).wrapping_mul(2654435761) >> (32 - HB as u32)) as usize };
    let mut i = 0;
    while i < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + 3 <= data.len() {
            let h = hash(i);
            let mut cand = head[h];
            let mut chain = 24;
            while cand >= 0 && chain > 0 {
                let c = cand as usize;
                let dist = i - c;
                if dist > 32768 {
                    break;
                }
                let maxl = (data.len() - i).min(258);
                let mut l = 0;
                while l < maxl && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = dist;
                    if l == maxl {
                        break;
                    }
                }
                cand = prev[c];
                chain -= 1;
            }
            prev[i] = head[h];
            head[h] = i as i32;
        }
        if best_len >= 3 {
            let li = LBASE.iter().rposition(|&b| b as usize <= best_len).unwrap_or(0);
            lit(&mut w, 257 + li as u32);
            w.put((best_len - LBASE[li] as usize) as u32, LEXT[li] as u32);
            let di = DBASE.iter().rposition(|&b| b as usize <= best_dist).unwrap_or(0);
            w.code(di as u32, 5);
            w.put((best_dist - DBASE[di] as usize) as u32, DEXT[di] as u32);
            // register skipped positions in the hash
            for k in 1..best_len {
                let p = i + k;
                if p + 3 <= data.len() {
                    let h = hash(p);
                    prev[p] = head[h];
                    head[h] = p as i32;
                }
            }
            i += best_len;
        } else {
            lit(&mut w, data[i] as u32);
            i += 1;
        }
    }
    lit(&mut w, 256);
    w.finish()
}

fn crc(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[n as usize] = c;
    }
    let mut c = 0xFFFFFFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 255) as usize] ^ (c >> 8);
    }
    !c
}

/// Encode an opaque RGB PNG of the canvas region (x,y,w,h). Uses sub filter per row for better compression.
pub fn encode(cv: &Canvas, rx: usize, ry: usize, rw: usize, rh: usize) -> Vec<u8> {
    let rw = rw.min(cv.w.saturating_sub(rx)).max(1);
    let rh = rh.min(cv.h.saturating_sub(ry)).max(1);
    let mut raw = Vec::with_capacity((rw * 3 + 1) * rh);
    for y in 0..rh {
        raw.push(1); // filter: Sub
        let mut prev = [0u8; 3];
        for x in 0..rw {
            let p = cv.px[(ry + y) * cv.w + rx + x];
            let px = [((p >> 16) & 255) as u8, ((p >> 8) & 255) as u8, (p & 255) as u8];
            for k in 0..3 {
                raw.push(px[k].wrapping_sub(prev[k]));
            }
            prev = px;
        }
    }
    let mut z = vec![0x78, 0x9C];
    z.extend_from_slice(&deflate(&raw));
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut chunk = |tag: &[u8; 4], d: &[u8]| {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut t = tag.to_vec();
        t.extend_from_slice(d);
        out.extend_from_slice(&t);
        out.extend_from_slice(&crc(&t).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(rw as u32).to_be_bytes());
    ihdr.extend_from_slice(&(rh as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
