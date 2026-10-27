//! DEFLATE (RFC1951) + PNG decoder. All indexing is bounds-checked; malformed data yields None.
use crate::gfx::Image;

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    cnt: u32,
}
impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) -> Option<()> {
        while self.cnt < n {
            let b = *self.d.get(self.pos)? as u32;
            self.pos += 1;
            self.bit |= b << self.cnt;
            self.cnt += 8;
        }
        Some(())
    }
    fn get(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.cnt -= n;
        Some(v)
    }
}

struct Huff {
    count: [u16; 16],
    sym: Vec<u16>,
}
fn build(lengths: &[u8]) -> Huff {
    let mut count = [0u16; 16];
    for &l in lengths {
        count[(l & 15) as usize] += 1;
    }
    count[0] = 0;
    let mut offs = [0u16; 16];
    for i in 1..16 {
        offs[i] = offs[i - 1] + count[i - 1];
    }
    let mut sym = vec![0u16; lengths.len()];
    for (s, &l) in lengths.iter().enumerate() {
        if l != 0 {
            sym[offs[l as usize] as usize] = s as u16;
            offs[l as usize] += 1;
        }
    }
    Huff { count, sym }
}
fn decode(b: &mut Bits, h: &Huff) -> Option<u16> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= b.get(1)? as i32;
        let c = h.count[len] as i32;
        if code - c < first {
            return h.sym.get((index + (code - first)) as usize).copied();
        }
        index += c;
        first += c;
        first <<= 1;
        code <<= 1;
    }
    None
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

pub fn inflate(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut b = Bits { d: data, pos: 0, bit: 0, cnt: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = b.get(1)?;
        let ty = b.get(2)?;
        match ty {
            0 => {
                b.bit = 0;
                b.cnt = 0;
                let len = u16::from_le_bytes([*b.d.get(b.pos)?, *b.d.get(b.pos + 1)?]) as usize;
                b.pos += 4;
                out.extend_from_slice(b.d.get(b.pos..b.pos + len)?);
                b.pos += len;
            }
            1 | 2 => {
                let (lh, dh);
                if ty == 1 {
                    let mut l = [0u8; 288];
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = if i < 144 { 8 } else if i < 256 { 9 } else if i < 280 { 7 } else { 8 };
                    }
                    lh = build(&l);
                    dh = build(&[5u8; 30]);
                } else {
                    let nlen = b.get(5)? as usize + 257;
                    let ndist = b.get(5)? as usize + 1;
                    let ncode = b.get(4)? as usize + 4;
                    const ORD: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for i in 0..ncode {
                        cl[ORD[i]] = b.get(3)? as u8;
                    }
                    let ch = build(&cl);
                    let mut lens = vec![0u8; nlen + ndist];
                    let mut i = 0;
                    while i < nlen + ndist {
                        let s = decode(&mut b, &ch)?;
                        if s < 16 {
                            lens[i] = s as u8;
                            i += 1;
                        } else {
                            let (v, rep) = match s {
                                16 => (*lens.get(i.checked_sub(1)?)?, 3 + b.get(2)?),
                                17 => (0, 3 + b.get(3)?),
                                _ => (0, 11 + b.get(7)?),
                            };
                            for _ in 0..rep {
                                *lens.get_mut(i)? = v;
                                i += 1;
                            }
                        }
                    }
                    lh = build(&lens[..nlen]);
                    dh = build(&lens[nlen..]);
                }
                loop {
                    let s = decode(&mut b, &lh)? as usize;
                    if s < 256 {
                        out.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let s = s - 257;
                        let len = *LBASE.get(s)? as usize + b.get(*LEXT.get(s)? as u32)? as usize;
                        let ds = decode(&mut b, &dh)? as usize;
                        let dist = *DBASE.get(ds)? as usize + b.get(*DEXT.get(ds)? as u32)? as usize;
                        if dist > out.len() {
                            return None;
                        }
                        let start = out.len() - dist;
                        for k in 0..len {
                            let v = out[start + k];
                            out.push(v);
                        }
                    }
                    if out.len() > limit {
                        return None;
                    }
                }
            }
            _ => return None,
        }
        if out.len() > limit {
            return None;
        }
        if last == 1 {
            break;
        }
    }
    Some(out)
}

pub fn decode_png(d: &[u8]) -> Option<Image> {
    if d.get(0..8)? != [0x89, b'P', b'N', b'G', 13, 10, 26, 10] {
        return None;
    }
    let mut p = 8;
    let (mut w, mut h, mut depth, mut ct, mut interlace) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let mut idat = Vec::new();
    let mut pal: Vec<u32> = vec![];
    let mut trns: Vec<u8> = vec![];
    while p + 8 <= d.len() {
        let len = u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
        let tag = d.get(p + 4..p + 8)?;
        let body = d.get(p + 8..p + 8 + len)?;
        match tag {
            b"IHDR" => {
                w = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                h = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                depth = body[8];
                ct = body[9];
                interlace = body[12];
            }
            b"PLTE" => pal = body.chunks_exact(3).map(|c| 0xFF000000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect(),
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    if w == 0 || h == 0 || w > 4096 || h > 4096 || interlace != 0 || idat.len() < 6 {
        return None;
    }
    let ch = match ct {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return None;
    }
    let bpp_bits = ch * depth as usize;
    let stride = (w * bpp_bits + 7) / 8;
    let raw = inflate(&idat[2..], (stride + 1) * h + 16)?;
    if raw.len() < (stride + 1) * h {
        return None;
    }
    let bpp = ((bpp_bits + 7) / 8).max(1);
    let mut img = vec![0u8; stride * h];
    for y in 0..h {
        let f = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= bpp { img[y * stride + x - bpp] as i32 } else { 0 };
            let b = if y > 0 { img[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if x >= bpp && y > 0 { img[(y - 1) * stride + x - bpp] as i32 } else { 0 };
            let v = src[x] as i32
                + match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => (a + b) / 2,
                    4 => {
                        let pp = a + b - c;
                        let (pa, pb, pc) = ((pp - a).abs(), (pp - b).abs(), (pp - c).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    }
                    _ => return None,
                };
            img[y * stride + x] = v as u8;
        }
    }
    let mut px = vec![0u32; w * h];
    let sample = |row: &[u8], i: usize| -> u32 {
        match depth {
            8 => row[i] as u32,
            16 => row[i * 2] as u32,
            _ => {
                let per = 8 / depth as usize;
                let byte = row[i / per];
                let shift = 8 - depth as usize * (i % per + 1);
                ((byte >> shift) & ((1 << depth) - 1)) as u32
            }
        }
    };
    let scale = |v: u32| -> u32 {
        match depth {
            1 => v * 255,
            2 => v * 85,
            4 => v * 17,
            _ => v,
        }
    };
    for y in 0..h {
        let row = &img[y * stride..(y + 1) * stride];
        for x in 0..w {
            px[y * w + x] = match ct {
                0 => {
                    let g = scale(sample(row, x));
                    let a = if trns.len() >= 2 && (u16::from_be_bytes([trns[0], trns[1]]) as u32) == sample(row, x) { 0 } else { 255 };
                    (a << 24) | (g << 16) | (g << 8) | g
                }
                2 => {
                    let (r, g, b) = (sample(row, x * 3), sample(row, x * 3 + 1), sample(row, x * 3 + 2));
                    0xFF000000 | (r << 16) | (g << 8) | b
                }
                3 => {
                    let i = sample(row, x) as usize;
                    let base = pal.get(i).copied().unwrap_or(0xFF000000);
                    let a = trns.get(i).copied().unwrap_or(255) as u32;
                    (a << 24) | (base & 0xFFFFFF)
                }
                4 => {
                    let g = sample(row, x * 2);
                    let a = sample(row, x * 2 + 1);
                    (a << 24) | (g << 16) | (g << 8) | g
                }
                _ => {
                    let (r, g, b, a) = (sample(row, x * 4), sample(row, x * 4 + 1), sample(row, x * 4 + 2), sample(row, x * 4 + 3));
                    (a << 24) | (r << 16) | (g << 8) | b
                }
            };
        }
    }
    Some(Image { w, h, px })
}
