//! Port of `src/nvim/sha256.c` (FIPS-180-2 SHA-256; not vendored under `vendor/`).
//!
//! `sha256_bytes` is referenced from the vendored eval tree (by `f_sha256`) so
//! the drift gate recognizes it; `sha256_start`/`_process`/`_update`/`_finish`
//! are only called within `sha256.c`, so they are allowlisted.
//!
//! The C `sha256_process` unrolls the 64 rounds with the round constants inlined
//! in each `P()` macro; the canonical equivalent here is the same rounds over the
//! standard `K` table (bit-identical, verified against the FIPS test vectors via
//! Neovim). The `GET_UINT32`/`PUT_UINT32` macros are big-endian word access
//! (`u32::from_be_bytes`/`to_be_bytes`); `sha256_self_test()` (validation only,
//! no effect on output) is omitted.
#![allow(non_snake_case, non_camel_case_types)]

/// `SHA256_BUFFER_SIZE` from `Src/sha256.h:7`.
pub const SHA256_BUFFER_SIZE: usize = 64;
/// `SHA256_SUM_SIZE` from `Src/sha256.h:8`.
pub const SHA256_SUM_SIZE: usize = 32;

/// Port of `context_sha256_T` from `Src/sha256.h`.
pub struct context_sha256_T {
    pub total: [u32; 2],
    pub state: [u32; 8],
    pub buffer: [u8; SHA256_BUFFER_SIZE],
}

/// FIPS-180-2 round constants (the `K` values inlined in the C `P()` calls).
const K: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];

/// `sha256_padding` from `Src/sha256.c:216` — a `0x80` byte then zeros.
const SHA256_PADDING: [u8; SHA256_BUFFER_SIZE] = {
    let mut p = [0u8; SHA256_BUFFER_SIZE];
    p[0] = 0x80;
    p
};

/// Port of `sha256_start()` from `Src/sha256.c:38`.
pub fn sha256_start(ctx: &mut context_sha256_T) {
    ctx.total[0] = 0;
    ctx.total[1] = 0;
    ctx.state = [
        0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
        0x5BE0CD19,
    ];
}

/// Port of `sha256_process()` from `Src/sha256.c:53` (one 64-byte block).
fn sha256_process(ctx: &mut context_sha256_T, data: &[u8; SHA256_BUFFER_SIZE]) {
    // c: S0/S1 = message-schedule sigmas; S2/S3 = compression sigmas; F0=Maj, F1=Ch.
    let s0 = |x: u32| x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
    let s1 = |x: u32| x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10);
    let big_s0 = |x: u32| x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22);
    let big_s1 = |x: u32| x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25);
    let f0 = |x: u32, y: u32, z: u32| (x & y) | (z & (x | y));
    let f1 = |x: u32, y: u32, z: u32| z ^ (x & (y ^ z));

    let mut w = [0u32; SHA256_BUFFER_SIZE];
    for i in 0..16 {
        // c: GET_UINT32(W[i], data, i*4) — big-endian.
        w[i] = u32::from_be_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
    }
    for t in 16..64 {
        // c: R(t): W[t] = S1(W[t-2]) + W[t-7] + S0(W[t-15]) + W[t-16]
        w[t] = s1(w[t - 2])
            .wrapping_add(w[t - 7])
            .wrapping_add(s0(w[t - 15]))
            .wrapping_add(w[t - 16]);
    }

    let mut a = ctx.state[0];
    let mut b = ctx.state[1];
    let mut c = ctx.state[2];
    let mut d = ctx.state[3];
    let mut e = ctx.state[4];
    let mut f = ctx.state[5];
    let mut g = ctx.state[6];
    let mut h = ctx.state[7];

    for t in 0..64 {
        // c: P(...): temp1 = h + S3(e) + F1(e,f,g) + K + W; temp2 = S2(a) + F0(a,b,c);
        //           d += temp1; h = temp1 + temp2;  (with the variables rotating)
        let temp1 = h
            .wrapping_add(big_s1(e))
            .wrapping_add(f1(e, f, g))
            .wrapping_add(K[t])
            .wrapping_add(w[t]);
        let temp2 = big_s0(a).wrapping_add(f0(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    ctx.state[0] = ctx.state[0].wrapping_add(a);
    ctx.state[1] = ctx.state[1].wrapping_add(b);
    ctx.state[2] = ctx.state[2].wrapping_add(c);
    ctx.state[3] = ctx.state[3].wrapping_add(d);
    ctx.state[4] = ctx.state[4].wrapping_add(e);
    ctx.state[5] = ctx.state[5].wrapping_add(f);
    ctx.state[6] = ctx.state[6].wrapping_add(g);
    ctx.state[7] = ctx.state[7].wrapping_add(h);
}

/// Port of `sha256_update()` from `Src/sha256.c:180`.
pub fn sha256_update(ctx: &mut context_sha256_T, input: &[u8]) {
    let mut length = input.len();
    if length == 0 {
        return;
    }
    let mut off = 0usize; // c advances the `input` pointer; here an index.
    let mut left = (ctx.total[0] & (SHA256_BUFFER_SIZE as u32 - 1)) as usize;

    let orig = length as u32;
    ctx.total[0] = ctx.total[0].wrapping_add(orig);
    if ctx.total[0] < orig {
        ctx.total[1] = ctx.total[1].wrapping_add(1);
    }

    let fill = SHA256_BUFFER_SIZE - left;
    if left != 0 && length >= fill {
        ctx.buffer[left..left + fill].copy_from_slice(&input[off..off + fill]);
        let block = ctx.buffer;
        sha256_process(ctx, &block);
        length -= fill;
        off += fill;
        left = 0;
    }
    while length >= SHA256_BUFFER_SIZE {
        let mut block = [0u8; SHA256_BUFFER_SIZE];
        block.copy_from_slice(&input[off..off + SHA256_BUFFER_SIZE]);
        sha256_process(ctx, &block);
        length -= SHA256_BUFFER_SIZE;
        off += SHA256_BUFFER_SIZE;
    }
    if length != 0 {
        ctx.buffer[left..left + length].copy_from_slice(&input[off..off + length]);
    }
}

/// Port of `sha256_finish()` from `Src/sha256.c:223`.
pub fn sha256_finish(ctx: &mut context_sha256_T, digest: &mut [u8; SHA256_SUM_SIZE]) {
    let high = (ctx.total[0] >> 29) | (ctx.total[1] << 3);
    let low = ctx.total[0] << 3;

    let mut msglen = [0u8; 8];
    msglen[0..4].copy_from_slice(&high.to_be_bytes());
    msglen[4..8].copy_from_slice(&low.to_be_bytes());

    let last = ctx.total[0] & 0x3F;
    let padn = if last < 56 { 56 - last } else { 120 - last };

    sha256_update(ctx, &SHA256_PADDING[..padn as usize]);
    sha256_update(ctx, &msglen);

    for i in 0..8 {
        digest[i * 4..i * 4 + 4].copy_from_slice(&ctx.state[i].to_be_bytes());
    }
}

/// Port of `sha256_bytes()` from `Src/sha256.c:259`.
///
/// Hex digest of `buf` (and `salt`, if present). The C returns a pointer into a
/// static buffer; here it returns an owned `String`.
pub fn sha256_bytes(buf: &[u8], salt: Option<&[u8]>) -> String {
    // c: sha256_self_test() — omitted (validation only, no effect on output).
    let mut ctx = context_sha256_T {
        total: [0; 2],
        state: [0; 8],
        buffer: [0; SHA256_BUFFER_SIZE],
    };
    sha256_start(&mut ctx);
    sha256_update(&mut ctx, buf);
    if let Some(salt) = salt {
        sha256_update(&mut ctx, salt);
    }
    let mut sha256sum = [0u8; SHA256_SUM_SIZE];
    sha256_finish(&mut ctx, &mut sha256sum);

    let mut hexit = String::with_capacity(SHA256_BUFFER_SIZE);
    for byte in &sha256sum {
        hexit.push_str(&format!("{byte:02x}"));
    }
    hexit
}
