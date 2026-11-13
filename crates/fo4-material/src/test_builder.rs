//! Shared synthetic bytes, authored in this workspace; no game payload.
#[derive(Default)]
pub(super) struct W {
    pub(super) v: Vec<u8>,
    pub(super) ranges: Vec<(usize, usize)>,
}
impl W {
    pub(super) fn bytes(&mut self, s: &[u8]) {
        let start = self.v.len();
        self.v.extend(s);
        self.ranges.push((start, self.v.len()));
    }
    pub(super) fn u(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub(super) fn b(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    pub(super) fn f(&mut self) {
        self.bytes(&1.25f32.to_le_bytes());
    }
    pub(super) fn fs(&mut self, n: usize) {
        for _ in 0..n {
            self.f();
        }
    }
    pub(super) fn bs(&mut self, n: usize, b: bool) {
        for _ in 0..n {
            self.b(b as u8);
        }
    }
    pub(super) fn path(&mut self, s: &[u8]) {
        self.u((s.len() + 1) as u32);
        let mut v = s.to_vec();
        v.push(0);
        self.bytes(&v);
    }
}
pub(super) fn fixture(kind: &[u8; 4], v: u32, on: bool, path: &[u8]) -> W {
    let mut w = W::default();
    w.bytes(kind);
    w.u(v);
    w.u(3);
    w.fs(5);
    w.b(1);
    w.u(6);
    w.u(7);
    w.b(128);
    w.b(0);
    w.bs(8, on);
    w.bs(2, on);
    w.f();
    if v < 10 {
        w.bs(1, on);
        w.f();
    } else {
        w.bs(1, on);
    }
    w.bs(1, on);
    if v >= 6 {
        w.b(63);
    }
    if kind == b"BGEM" {
        for _ in 0..5 {
            w.path(path);
        }
        if v >= 11 {
            for _ in 0..3 {
                w.path(path);
            }
        }
        if v >= 21 {
            w.path(path);
            w.path(path);
            w.bs(1, on);
            if on {
                w.fs(4);
                if v >= 22 {
                    w.f();
                }
                w.f();
            }
        }
        if v >= 10 {
            w.bs(1, on);
            w.f();
        }
        w.bs(6, on);
        w.fs(9);
        w.b(3);
        w.f();
        if v >= 11 {
            w.fs(3);
        }
        if v >= 15 {
            w.fs(3);
        }
        if v >= 16 {
            w.bs(1, on);
        }
        if v >= 20 {
            w.bs(1, on);
        }
    } else {
        for _ in 0..9 {
            w.path(path);
        }
        if v >= 17 {
            w.path(path);
        }
        w.bs(1, on);
        if v >= 8 {
            w.bs(3, on);
            w.fs(5);
        } else {
            w.bs(1, on);
            w.fs(2);
            w.bs(1, on);
            w.f();
        }
        w.bs(1, on);
        w.fs(9);
        if v < 10 {
            w.f();
        }
        w.fs(2);
        if v > 2 {
            w.bs(1, on);
            if v >= 9 {
                w.bs(1, on);
                w.f();
            }
        }
        w.path(b"");
        w.bs(1, on);
        w.bs(1, on);
        if on {
            w.fs(3);
        }
        w.f();
        w.bs(2, on);
        if v >= 12 {
            w.f();
        }
        if v >= 13 {
            w.bs(1, on);
            w.fs(3);
        }
        if v < 8 {
            w.bs(1, on);
        }
        w.bs(6, on);
        if v < 7 {
            w.bs(2, on);
        }
        w.bs(1, on);
        w.fs(3);
        w.bs(4, on);
        if v < 3 {
            w.fs(5);
        }
        w.f();
        if v >= 1 {
            w.bs(1, on);
        }
        if v >= 3 {
            w.bs(1, on);
            if on {
                if v == 3 {
                    w.u(0x12345678);
                }
                w.fs(3);
            }
        }
    }
    w
}
