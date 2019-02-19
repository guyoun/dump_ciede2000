// BSD 2-Clause License
//
// Copyright (c) 2019, the dump_ciede2000 contributors
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// * Redistributions of source code must retain the above copyright notice, this
//  list of conditions and the following disclaimer.
//
// * Redistributions in binary form must reproduce the above copyright notice,
//  this list of conditions and the following disclaimer in the documentation
//  and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

extern crate clap;

#[macro_use]
extern crate itertools;
use itertools::Itertools;

use clap::{App, Arg, ArgMatches};
use std::fs::File;
use std::io::prelude::*;

use std::process::exit;

mod rgbtolab;
use rgbtolab::*;

mod delta_e;
use delta_e::*;

pub struct CliOptions {
    pub input1: Box<dyn Read>,
    pub input2: Box<dyn Read>,
    pub summary: bool,
    pub limit: Option<usize>,
}

fn parse_cli() -> CliOptions {
    let matches = App::new("fast_ciede2000")
        .about("Video quality metric based off color difference instead of just luma or chroma")
        .arg(
            Arg::with_name("video1")
                .help("Uncompressed YUV4MPEG2 video input")
                .required(true),
        )
        .arg(
            Arg::with_name("video2")
                .help("Uncompressed YUV4MPEG2 video input")
                .required(true),
        )
        .arg(
            Arg::with_name("LIMIT")
                .help("Only output the summary line")
                .short("l")
                .long("limit"),
        )
        .arg(
            Arg::with_name("SUMMARY")
                .help("Only output the summary line")
                .short("s")
                .long("summary"),
        )
        .get_matches();
    CliOptions {
        input1: Box::new(File::open(matches.value_of("video1").unwrap()).unwrap()) as Box<dyn Read>,
        input2: Box::new(File::open(matches.value_of("video2").unwrap()).unwrap()) as Box<dyn Read>,
        summary: matches.is_present("SUMMARY"),
        limit: matches
            .value_of("LIMIT")
            .map(|v| v.parse().expect("Limit must be a positive number")),
    }
}

fn main() {
    let mut cli = parse_cli();
    let mut video1 = y4m::decode(&mut cli.input1).unwrap();
    let mut video2 = y4m::decode(&mut cli.input2).unwrap();
    let (width, height) = {
        let dimension1 = (video1.get_width(), video1.get_height());
        let dimension2 = (video2.get_width(), video2.get_height());

        if dimension1 != dimension2 {
            eprintln!(
                "Video dimensions do not match: {}x{} != {}x{}",
                dimension1.0, dimension1.1, dimension2.0, dimension2.1
            );
            exit(1);
        }
        dimension1
    };
    let (bit_depth, bytewidth) = {
        /*let colorspace1 = video1.get_colorspace();
        let colorspace2 = video2.get_colorspace();*/
        let bit_depth1 = video1.get_bit_depth();
        let bit_depth2 = video2.get_bit_depth();
        if bit_depth1 != bit_depth2 {
            eprintln!("Bit depths do not match: {} != {}", bit_depth1, bit_depth2);
            exit(1);
        }
        // TODO: get and test chroma sampling
        (bit_depth1, video1.get_bytes_per_sample())
    };
    {
        let framerate1 = video1.get_framerate();
        let framerate2 = video2.get_framerate();
        if framerate1.num * framerate2.den != framerate2.num * framerate1.den {
            eprintln!(
                "Warning - Framerates do not match: {} != {}",
                framerate1, framerate2
            );
        }
    }

    //let y_stride = width * bytewidth;
    let sample_max = (1 << bit_depth) - 1;
    let mut num_frames: usize = 0;
    let mut total: f64 = 0f64;
    loop {
        match (video1.read_frame(), video2.read_frame()) {
            (Ok(pic1), Ok(pic2)) => {
                let mut delta_e_vec: Vec<f32> = vec![0.0; width * height];
                let y_plane1 = pic1.get_y_plane();
                let u_plane1 = pic1.get_u_plane();
                let v_plane1 = pic1.get_v_plane();
                let y_plane2 = pic2.get_y_plane();
                let u_plane2 = pic2.get_u_plane();
                let v_plane2 = pic2.get_v_plane();
                for i in 0..height {
                    match bytewidth {
                        1 => {
                            delta_e_row(
                                FrameRow {
                                    y: &y_plane1[i * width..][..width],
                                    u: &u_plane1[(i >> 1) * (width >> 1)..][..width >> 1],
                                    v: &v_plane1[(i >> 1) * (width >> 1)..][..width >> 1],
                                },
                                FrameRow {
                                    y: &y_plane2[i * width..][..width],
                                    u: &u_plane2[(i >> 1) * (width >> 1)..][..width >> 1],
                                    v: &v_plane2[(i >> 1) * (width >> 1)..][..width >> 1],
                                },
                                &mut delta_e_vec[i * width..][..width],
                            );
                        }
                        _ => {}
                    }
                }
                let score = 45.
                    - 20.
                        * (delta_e_vec.iter().map(|x| *x as f64).sum::<f64>()
                            / ((width * height) as f64))
                            .log10();
                println!("{:08}: {:2.4}", num_frames, score);
                total += score;
                num_frames += 1;
                if let Some(limit) = cli.limit {
                    if num_frames >= limit {
                        break;
                    }
                }
            }
            _ => {
                break;
            }
        }
    }
    println!("Total: {:2.4}", total / (num_frames as f64));
}

// Arguments for delta e
// "Color Image Quality Assessment Based on CIEDE2000"
// Yang Yang, Jun Ming and Nenghai Yu, 2012
// http://dx.doi.org/10.1155/2012/273723
const K_SUB: KSubArgs = KSubArgs {
    l: 0.65,
    c: 1.0,
    h: 4.0,
};

struct FrameRow<'a> {
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
}

fn delta_e_row(row1: FrameRow, row2: FrameRow, res_row: &mut [f32]) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { delta_e_row_avx2(row1, row2, res_row) };
        }
    }
    delta_e_row_scalar(row1, row2, res_row);
}

fn delta_e_row_scalar(row1: FrameRow, row2: FrameRow, res_row: &mut [f32]) {
    for (y1, u1, v1, y2, u2, v2, res) in izip!(
        row1.y,
        row1.u.iter().interleave(row1.u.iter()),
        row1.v.iter().interleave(row1.v.iter()),
        row2.y,
        row2.u.iter().interleave(row2.u.iter()),
        row2.v.iter().interleave(row2.v.iter()),
        res_row
    ) {
        let yuv_to_rgb = |y: f32, u: f32, v: f32| {
            let y = (y - 16.) * (1. / 219.);
            let u = (u - 128.) * (1. / 224.);
            let v = (v - 128.) * (1. / 224.);

            let r = y + 1.28033 * v;
            let g = y - 0.21482 * u - 0.38059 * v;
            let b = y + 2.12798 * u;

            (r, g, b)
        };

        let (r1, g1, b1) = yuv_to_rgb(*y1 as f32, *u1 as f32, *v1 as f32);
        let (r2, g2, b2) = yuv_to_rgb(*y2 as f32, *u2 as f32, *v2 as f32);
        *res = DE2000::new(rgb_to_lab(&[r1, g1, b1]), rgb_to_lab(&[r2, g2, b2]), K_SUB);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn delta_e_row_avx2(row1: FrameRow, row2: FrameRow, res_row: &mut [f32]) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    for (chunk1_y, chunk1_u, chunk1_v, chunk2_y, chunk2_u, chunk2_v, res_chunk) in izip!(
        row1.y.chunks(8),
        row1.u.chunks(4),
        row1.v.chunks(4),
        row2.y.chunks(8),
        row2.u.chunks(4),
        row2.v.chunks(4),
        res_row.chunks_mut(8)
    ) {
        if chunk1_y.len() == 8 {
            #[target_feature(enable = "avx2")]
            unsafe fn load_luma(chunk: &[u8]) -> __m256 {
                let tmp = _mm_loadl_epi64(chunk.as_ptr() as *const _);
                _mm256_cvtepi32_ps(_mm256_cvtepu8_epi32(tmp))
            };

            #[target_feature(enable = "avx2")]
            unsafe fn load_chroma(chunk: &[u8]) -> __m256 {
                let tmp = _mm_cvtsi32_si128(*(chunk.as_ptr() as *const i32));
                _mm256_cvtepi32_ps(_mm256_cvtepu8_epi32(_mm_unpacklo_epi8(tmp, tmp)))
            };

            #[target_feature(enable = "avx2")]
            unsafe fn yuv_to_rgb(y: __m256, u: __m256, v: __m256) -> (__m256, __m256, __m256) {
                #[target_feature(enable = "avx2")]
                unsafe fn set1(val: f32) -> __m256 {
                    _mm256_set1_ps(val)
                };
                let y = _mm256_mul_ps(_mm256_sub_ps(y, set1(16.)), set1(1. / 219.));
                let u = _mm256_mul_ps(_mm256_sub_ps(u, set1(128.)), set1(1. / 224.));
                let v = _mm256_mul_ps(_mm256_sub_ps(v, set1(128.)), set1(1. / 224.));

                let r = _mm256_add_ps(y, _mm256_mul_ps(v, set1(1.28033)));
                let g = _mm256_add_ps(
                    _mm256_add_ps(y, _mm256_mul_ps(u, set1(-0.21482))),
                    _mm256_mul_ps(v, set1(-0.38059)),
                );
                let b = _mm256_add_ps(y, _mm256_mul_ps(u, set1(2.12798)));

                (r, g, b)
            };

            let (r1, g1, b1) = yuv_to_rgb(
                load_luma(chunk1_y),
                load_chroma(chunk1_u),
                load_chroma(chunk1_v),
            );
            let (r2, g2, b2) = yuv_to_rgb(
                load_luma(chunk2_y),
                load_chroma(chunk2_u),
                load_chroma(chunk2_v),
            );

            let lab1 = rgb_to_lab_avx2(&[r1, g1, b1]);
            let lab2 = rgb_to_lab_avx2(&[r2, g2, b2]);
            for i in 0..8 {
                res_chunk[i] = DE2000::new(lab1[i], lab2[i], K_SUB);
            }
        } else {
            delta_e_row_scalar(
                FrameRow {
                    y: chunk1_y,
                    u: chunk1_u,
                    v: chunk1_v,
                },
                FrameRow {
                    y: chunk2_y,
                    u: chunk2_u,
                    v: chunk2_v,
                },
                res_chunk,
            );
        }
    }
}
