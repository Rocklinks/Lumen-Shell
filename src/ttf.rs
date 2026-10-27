//! TrueType (glyf) parser + glyph outline extraction. Every read is bounds-checked (Option),
//! so a corrupt font can never panic the shell. CFF/OTF-CFF fonts are rejected (caller falls back).
use std::collections::HashMap;

pub struct Font {
    data: Vec<u8>,
    tables: HashMap<[u8; 4], (usize, usize)>,
    pub units: f32,
    loca_long: bool,
    num_glyphs: u16,
    num_hmetrics: u16,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    cmap: (usize, u16), // offset, format
}

pub struct Pt {
    pub x: f32,
    pub y: f32,
    pub on: bool,
}
pub type Contour = Vec<Pt>;

impl Font {
    fn u16at(&self, o: usize) -> Option<u16> {
        let b = self.data.get(o..o.checked_add(2)?)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16at(&self, o: usize) -> Option<i16> {
        self.u16at(o).map(|v| v as i16)
    }
    fn u32at(&self, o: usize) -> Option<u32> {
        let b = self.data.get(o..o.checked_add(4)?)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn table(&self, tag: &[u8; 4]) -> Option<usize> {
        self.tables.get(tag).map(|t| t.0)
    }

    pub fn parse(data: Vec<u8>) -> Option<Font> {
        let mut f = Font {
            data,
            tables: HashMap::new(),
            units: 1000.0,
            loca_long: false,
            num_glyphs: 0,
            num_hmetrics: 0,
            ascent: 0.0,
            descent: 0.0,
            line_gap: 0.0,
            cmap: (0, 0),
        };
        let mut base = 0usize;
        let tag = f.u32at(0)?;
        if tag == 0x74746366 {
            base = f.u32at(12)? as usize; // ttcf: first font
        }
        let ver = f.u32at(base)?;
        if ver != 0x00010000 && ver != 0x74727565 {
            return None; // OTTO (CFF) unsupported
        }
        let n = f.u16at(base + 4)? as usize;
        for i in 0..n {
            let r = base + 12 + i * 16;
            let t = f.data.get(r..r + 4)?;
            let tg = [t[0], t[1], t[2], t[3]];
            let off = f.u32at(r + 8)? as usize;
            let len = f.u32at(r + 12)? as usize;
            if off.checked_add(len)? > f.data.len() {
                continue;
            }
            f.tables.insert(tg, (off, len));
        }
        let head = f.table(b"head")?;
        f.units = f.u16at(head + 18)? as f32;
        if f.units < 16.0 {
            return None;
        }
        f.loca_long = f.i16at(head + 50)? != 0;
        f.num_glyphs = f.u16at(f.table(b"maxp")? + 4)?;
        let hhea = f.table(b"hhea")?;
        f.ascent = f.i16at(hhea + 4)? as f32;
        f.descent = f.i16at(hhea + 6)? as f32;
        f.line_gap = f.i16at(hhea + 8)? as f32;
        f.num_hmetrics = f.u16at(hhea + 34)?;
        f.table(b"hmtx")?;
        f.table(b"loca")?;
        f.table(b"glyf")?;
        // cmap: prefer (3,10), (3,1), (0,*)
        let cm = f.table(b"cmap")?;
        let nt = f.u16at(cm + 2)? as usize;
        let mut best: Option<(usize, u32)> = None;
        for i in 0..nt {
            let r = cm + 4 + i * 8;
            let (p, e) = (f.u16at(r)?, f.u16at(r + 2)?);
            let off = cm + f.u32at(r + 4)? as usize;
            let fmt = f.u16at(off)?;
            if fmt != 4 && fmt != 12 {
                continue;
            }
            let score = match (p, e) {
                (3, 10) => 4,
                (0, 4) | (0, 6) => 4,
                (3, 1) => 3,
                (0, _) => 2,
                _ => 0,
            };
            if score > 0 && best.map_or(true, |b| score > b.1) {
                best = Some((off, score));
            }
        }
        let (off, _) = best?;
        f.cmap = (off, f.u16at(off)?);
        Some(f)
    }

    pub fn glyph_index(&self, ch: char) -> u16 {
        self.glyph_index_opt(ch).unwrap_or(0)
    }

    fn glyph_index_opt(&self, ch: char) -> Option<u16> {
        let c = ch as u32;
        let (off, fmt) = self.cmap;
        if fmt == 4 {
            if c > 0xFFFF {
                return None;
            }
            let segx2 = self.u16at(off + 6)? as usize;
            let segs = segx2 / 2;
            let ends = off + 14;
            let starts = ends + segx2 + 2;
            let deltas = starts + segx2;
            let ranges = deltas + segx2;
            // binary search over end codes
            let (mut lo, mut hi) = (0usize, segs);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if (self.u16at(ends + mid * 2)? as u32) < c {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if lo >= segs {
                return None;
            }
            let start = self.u16at(starts + lo * 2)? as u32;
            if c < start {
                return None;
            }
            let delta = self.u16at(deltas + lo * 2)?;
            let ro = self.u16at(ranges + lo * 2)? as usize;
            if ro == 0 {
                Some((c as u16).wrapping_add(delta))
            } else {
                let p = ranges + lo * 2 + ro + (c - start) as usize * 2;
                let g = self.u16at(p)?;
                if g == 0 {
                    None
                } else {
                    Some(g.wrapping_add(delta))
                }
            }
        } else {
            let ng = self.u32at(off + 12)? as usize;
            let (mut lo, mut hi) = (0usize, ng.min(100_000));
            while lo < hi {
                let mid = (lo + hi) / 2;
                let g = off + 16 + mid * 12;
                let (s, e, sg) = (self.u32at(g)?, self.u32at(g + 4)?, self.u32at(g + 8)?);
                if c < s {
                    hi = mid;
                } else if c > e {
                    lo = mid + 1;
                } else {
                    return Some((sg + (c - s)) as u16);
                }
            }
            None
        }
    }

    pub fn has_glyph(&self, ch: char) -> bool {
        self.glyph_index_opt(ch).map_or(false, |g| g != 0)
    }

    pub fn advance(&self, gid: u16) -> f32 {
        let hm = match self.table(b"hmtx") {
            Some(h) => h,
            None => return self.units * 0.5,
        };
        let i = (gid.min(self.num_hmetrics.saturating_sub(1))) as usize;
        self.u16at(hm + i * 4).unwrap_or(0) as f32
    }

    fn glyph_range(&self, gid: u16) -> Option<(usize, usize)> {
        if gid >= self.num_glyphs {
            return None;
        }
        let loca = self.table(b"loca")?;
        let glyf = self.table(b"glyf")?;
        let (a, b) = if self.loca_long {
            (self.u32at(loca + gid as usize * 4)? as usize, self.u32at(loca + gid as usize * 4 + 4)? as usize)
        } else {
            (self.u16at(loca + gid as usize * 2)? as usize * 2, self.u16at(loca + gid as usize * 2 + 2)? as usize * 2)
        };
        if b <= a {
            return None; // empty glyph (space)
        }
        Some((glyf + a, glyf + b))
    }

    /// Outline in font units (y up). Empty vec for blank glyphs.
    pub fn outline(&self, gid: u16) -> Vec<Contour> {
        let mut out = Vec::new();
        let _ = self.outline_into(gid, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, &mut out);
        out
    }

    fn outline_into(&self, gid: u16, m: [f32; 6], depth: u32, out: &mut Vec<Contour>) -> Option<()> {
        if depth > 6 {
            return None;
        }
        let (start, end) = match self.glyph_range(gid) {
            Some(r) => r,
            None => return Some(()),
        };
        let nc = self.i16at(start)?;
        if nc >= 0 {
            let nc = nc as usize;
            let mut p = start + 10;
            let mut ends = Vec::with_capacity(nc);
            for i in 0..nc {
                ends.push(self.u16at(p + i * 2)? as usize);
            }
            p += nc * 2;
            let npts = ends.last().map_or(0, |e| e + 1);
            if npts > 20000 {
                return None;
            }
            let ilen = self.u16at(p)? as usize;
            p += 2 + ilen;
            let mut flags = Vec::with_capacity(npts);
            while flags.len() < npts {
                let f = *self.data.get(p)?;
                p += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let r = *self.data.get(p)?;
                    p += 1;
                    for _ in 0..r {
                        if flags.len() < npts {
                            flags.push(f);
                        }
                    }
                }
            }
            let mut xs = Vec::with_capacity(npts);
            let mut v = 0i32;
            for &f in &flags {
                if f & 2 != 0 {
                    let d = *self.data.get(p)? as i32;
                    p += 1;
                    v += if f & 16 != 0 { d } else { -d };
                } else if f & 16 == 0 {
                    v += self.i16at(p)? as i32;
                    p += 2;
                }
                xs.push(v);
            }
            let mut ys = Vec::with_capacity(npts);
            v = 0;
            for &f in &flags {
                if f & 4 != 0 {
                    let d = *self.data.get(p)? as i32;
                    p += 1;
                    v += if f & 32 != 0 { d } else { -d };
                } else if f & 32 == 0 {
                    v += self.i16at(p)? as i32;
                    p += 2;
                }
                ys.push(v);
            }
            let mut s = 0usize;
            for &e in &ends {
                if e < s || e >= npts {
                    return None;
                }
                let mut c = Vec::with_capacity(e - s + 1);
                for i in s..=e {
                    let (x, y) = (xs[i] as f32, ys[i] as f32);
                    c.push(Pt {
                        x: m[0] * x + m[2] * y + m[4],
                        y: m[1] * x + m[3] * y + m[5],
                        on: flags[i] & 1 != 0,
                    });
                }
                out.push(c);
                s = e + 1;
            }
        } else {
            // composite
            let mut p = start + 10;
            loop {
                if p + 4 > end {
                    return None;
                }
                let fl = self.u16at(p)?;
                let gi = self.u16at(p + 2)?;
                p += 4;
                let (dx, dy);
                if fl & 1 != 0 {
                    dx = self.i16at(p)? as f32;
                    dy = self.i16at(p + 2)? as f32;
                    p += 4;
                } else {
                    dx = *self.data.get(p)? as i8 as f32;
                    dy = *self.data.get(p + 1)? as i8 as f32;
                    p += 2;
                }
                let (dx, dy) = if fl & 2 != 0 { (dx, dy) } else { (0.0, 0.0) };
                let f2 = |s: &Font, o: usize| s.i16at(o).map(|v| v as f32 / 16384.0);
                let (mut a, mut b, mut c, mut d) = (1.0, 0.0, 0.0, 1.0);
                if fl & 8 != 0 {
                    a = f2(self, p)?;
                    d = a;
                    p += 2;
                } else if fl & 0x40 != 0 {
                    a = f2(self, p)?;
                    d = f2(self, p + 2)?;
                    p += 4;
                } else if fl & 0x80 != 0 {
                    a = f2(self, p)?;
                    b = f2(self, p + 2)?;
                    c = f2(self, p + 4)?;
                    d = f2(self, p + 6)?;
                    p += 8;
                }
                // child matrix then parent matrix
                let cm = [a, b, c, d, dx, dy];
                let nm = [
                    m[0] * cm[0] + m[2] * cm[1],
                    m[1] * cm[0] + m[3] * cm[1],
                    m[0] * cm[2] + m[2] * cm[3],
                    m[1] * cm[2] + m[3] * cm[3],
                    m[0] * cm[4] + m[2] * cm[5] + m[4],
                    m[1] * cm[4] + m[3] * cm[5] + m[5],
                ];
                self.outline_into(gi, nm, depth + 1, out)?;
                if fl & 0x20 == 0 {
                    break;
                }
            }
        }
        Some(())
    }
}
