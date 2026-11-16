//! Golden-output net for the voices. Each voice renders a fixed input through the
//! same blocks and must produce the same bits as when the table below was made, so any
//! change that moves a single sample of any voice fails here. The hash is FNV-1a over
//! the samples' bit patterns. The sine uses the platform's libm, so the table is for
//! the Linux build that CI runs.

use goby_dsp::dsp::{Recipe, Voice};
use goby_dsp::fx::Noise;

const SR: u32 = 48_000;
const BLOCK: usize = 480;

/// Two seconds: a seeded noise burst from 0.5 s to 1 s over a sine sweep from 100 to 4000 Hz.
fn input() -> Vec<f32> {
    let mut noise = Noise::default();
    (0..2 * SR as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let burst = if (0.5..1.0).contains(&t) { 0.5 * noise.next() } else { 0.0 };
            let phase = std::f32::consts::TAU * (100.0 * t + 975.0 * t * t);
            burst + 0.4 * phase.sin()
        })
        .collect()
}

fn fnv1a(samples: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn render(recipe: &Recipe) -> u64 {
    let mut voice = Voice::new(SR);
    voice.set(recipe, 0.0, 1.0);
    let mut x = input();
    for block in x.chunks_mut(BLOCK) {
        voice.process(block);
    }
    fnv1a(&x)
}

/// Voice name, recipe as voices.rs writes it, and the golden hash.
const CASES: &[(&str, Recipe, u64)] = &[
    ("off", Recipe::DRY, 0x801e46dcbd8bd063),
    ("minion", Recipe { pitch: 8.0, ..Recipe::DRY }, 0x13a25d488d331fae),
    ("helium", Recipe { pitch: 12.0, ..Recipe::DRY }, 0x56277e0ded87cccb),
    (
        "giant",
        Recipe { pitch: -6.0, reverb_size: 0.3, reverb_damp: 0.5, reverb_mix: 0.12, ..Recipe::DRY },
        0xcb8c3b95ca3c6aae,
    ),
    (
        "robot",
        Recipe { ring_hz: 55.0, ring_mix: 1.0, comb_ms: 6.0, comb_feedback: 0.55, gain: 1.4, ..Recipe::DRY },
        0x93dab28286790593,
    ),
    (
        "radio",
        Recipe { highpass_hz: 450.0, lowpass_hz: 2800.0, drive: 4.0, gain: 1.2, ..Recipe::DRY },
        0xc85ca9e058b06c87,
    ),
    ("kid", Recipe { pitch: 5.0, chorus: 0.15, ..Recipe::DRY }, 0xe03145cca6e19ef2),
    ("baby", Recipe { pitch: 9.0, tremolo_hz: 6.0, tremolo_depth: 0.15, ..Recipe::DRY }, 0xc7b936cb8d87cd08),
    ("woman", Recipe { pitch: 5.0, formant: 1.15, highpass_hz: 120.0, gain: 1.2, ..Recipe::DRY }, 0xc192847bfe5ed738),
    ("man", Recipe { pitch: -5.0, formant: 0.87, gain: 1.2, ..Recipe::DRY }, 0x5257f646355828ff),
    (
        "bass",
        Recipe { pitch: -7.0, lowpass_hz: 6000.0, reverb_size: 0.2, reverb_damp: 0.6, reverb_mix: 0.1, ..Recipe::DRY },
        0x07bea144ef571728,
    ),
    (
        "grandpa",
        Recipe { pitch: -2.0, vibrato_hz: 5.0, vibrato_cents: 18.0, lowpass_hz: 4000.0, ..Recipe::DRY },
        0x39857d05c2aa0ec8,
    ),
    (
        "grandma",
        Recipe { pitch: 2.0, vibrato_hz: 5.5, vibrato_cents: 28.0, highpass_hz: 200.0, ..Recipe::DRY },
        0xc20552c0e5f92889,
    ),
    ("teen", Recipe { pitch: 2.0, wobble_hz: 2.0, wobble_cents: 45.0, ..Recipe::DRY }, 0xab9d1d10d2d116c9),
    (
        "pirate",
        Recipe { pitch: -4.0, drive: 2.0, reverb_size: 0.25, reverb_damp: 0.5, reverb_mix: 0.12, ..Recipe::DRY },
        0x55a1d9323726fe92,
    ),
    (
        "nervous",
        Recipe { vibrato_hz: 7.0, vibrato_cents: 35.0, tremolo_hz: 7.0, tremolo_depth: 0.2, ..Recipe::DRY },
        0xc584e3a5fc8dc4ca,
    ),
    (
        "drunk",
        Recipe {
            pitch: -1.0,
            wobble_hz: 1.2,
            wobble_cents: 70.0,
            echo_ms: 140.0,
            echo_feedback: 0.2,
            echo_mix: 0.2,
            ..Recipe::DRY
        },
        0x8a28c00c4a8ba356,
    ),
    ("whisper", Recipe { whisper: 1.0, highpass_hz: 300.0, gain: 1.6, ..Recipe::DRY }, 0xe0dddb2dffaa6097),
    (
        "sleepy",
        Recipe { pitch: -2.0, lowpass_hz: 3500.0, tremolo_hz: 0.5, tremolo_depth: 0.25, ..Recipe::DRY },
        0xc7e3d53cd58ed305,
    ),
    (
        "announcer",
        Recipe {
            pitch: -2.0,
            drive: 1.5,
            echo_ms: 260.0,
            echo_feedback: 0.25,
            echo_mix: 0.2,
            reverb_size: 0.7,
            reverb_damp: 0.4,
            reverb_mix: 0.2,
            ..Recipe::DRY
        },
        0x33a6740529f3ad51,
    ),
    ("strongman", Recipe { pitch: -3.0, formant: 0.82, drive: 1.3, ..Recipe::DRY }, 0xa88d7f1c294fc5ee),
    (
        "trailer",
        Recipe {
            pitch: -3.0,
            formant: 0.88,
            bass_db: 5.0,
            squash: 6.0,
            reverb_size: 0.35,
            reverb_damp: 0.5,
            reverb_mix: 0.12,
            gain: 2.2,
            ..Recipe::DRY
        },
        0x897ce20eb4919087,
    ),
    (
        "villain",
        Recipe {
            pitch: -2.0,
            formant: 0.9,
            harmony: [-12.0, 0.0],
            harmony_mix: 0.25,
            reverb_size: 0.7,
            reverb_damp: 0.7,
            reverb_mix: 0.2,
            ..Recipe::DRY
        },
        0xa0f12f5d9624880d,
    ),
    (
        "commentator",
        Recipe {
            eq_hz: 2500.0,
            eq_db: 6.0,
            squash: 4.0,
            drive: 2.0,
            hiss: 0.03,
            echo_ms: 120.0,
            echo_feedback: 0.2,
            echo_mix: 0.2,
            gain: 1.25,
            ..Recipe::DRY
        },
        0x40678bcd64ea37e2,
    ),
    ("gremlin", Recipe { pitch: 6.0, ring_hz: 90.0, ring_mix: 0.15, drive: 1.5, ..Recipe::DRY }, 0x32de6c4de5cdda19),
    ("rodent", Recipe { pitch: 10.0, chorus: 0.3, ..Recipe::DRY }, 0xedde0b490d414b43),
    ("ogre", Recipe { pitch: -6.0, drive: 2.5, lowpass_hz: 5000.0, ..Recipe::DRY }, 0xc882622ad9b28bbe),
    (
        "troll",
        Recipe {
            pitch: -7.0,
            ring_hz: 30.0,
            ring_mix: 0.3,
            reverb_size: 0.3,
            reverb_damp: 0.5,
            reverb_mix: 0.15,
            ..Recipe::DRY
        },
        0x59eba6ecd92abffa,
    ),
    (
        "demon",
        Recipe {
            pitch: -8.0,
            harmony: [-12.0, 0.0],
            harmony_mix: 0.45,
            drive: 2.5,
            reverb_size: 0.8,
            reverb_damp: 0.4,
            reverb_mix: 0.25,
            ..Recipe::DRY
        },
        0x3578295fec1b94f6,
    ),
    (
        "zombie",
        Recipe {
            pitch: -3.0,
            vibrato_hz: 3.0,
            vibrato_cents: 25.0,
            crush_hz: 12000.0,
            crush_bits: 8,
            lowpass_hz: 2500.0,
            gain: 1.3,
            ..Recipe::DRY
        },
        0xf14e53e5e384e312,
    ),
    (
        "ghost",
        Recipe {
            pitch: 1.0,
            whisper: 0.35,
            flanger_hz: 0.2,
            flanger_mix: 0.5,
            reverb_size: 0.75,
            reverb_damp: 0.35,
            reverb_mix: 0.4,
            ..Recipe::DRY
        },
        0x57ee174ebbee4043,
    ),
    (
        "possessed",
        Recipe {
            harmony: [-5.0, 4.0],
            harmony_mix: 0.5,
            ring_hz: 60.0,
            ring_mix: 0.15,
            reverb_size: 0.8,
            reverb_damp: 0.5,
            reverb_mix: 0.3,
            ..Recipe::DRY
        },
        0xefd3cd7b106a7520,
    ),
    (
        "vampire",
        Recipe { pitch: -3.0, chorus: 0.3, reverb_size: 0.8, reverb_damp: 0.3, reverb_mix: 0.2, ..Recipe::DRY },
        0x764ea545af50d85e,
    ),
    (
        "dragon",
        Recipe {
            pitch: -10.0,
            harmony: [-5.0, 0.0],
            harmony_mix: 0.4,
            drive: 3.0,
            reverb_size: 0.9,
            reverb_damp: 0.5,
            reverb_mix: 0.25,
            ..Recipe::DRY
        },
        0x92eb406429bfa6b6,
    ),
    (
        "swarm",
        Recipe { ring_hz: 110.0, ring_mix: 0.4, tremolo_hz: 20.0, tremolo_depth: 0.4, ..Recipe::DRY },
        0x093fd20633171346,
    ),
    (
        "alien",
        Recipe { pitch: 3.0, ring_hz: 250.0, ring_mix: 0.3, flanger_hz: 1.5, flanger_mix: 0.4, ..Recipe::DRY },
        0xcd704c5a8444b682,
    ),
    ("android", Recipe { vocoder_hz: 110.0, gain: 1.4, ..Recipe::DRY }, 0x203d8228d8e0a137),
    (
        "dictator",
        Recipe {
            ring_hz: 30.0,
            ring_mix: 1.0,
            highpass_hz: 400.0,
            lowpass_hz: 3400.0,
            drive: 3.0,
            hard_clip: true,
            gain: 1.4,
            ..Recipe::DRY
        },
        0xb4aafe58ee14ffad,
    ),
    (
        "cyborg",
        Recipe {
            pitch: -4.0,
            comb_ms: 3.0,
            comb_feedback: 0.6,
            reverb_size: 0.3,
            reverb_damp: 0.4,
            reverb_mix: 0.12,
            ..Recipe::DRY
        },
        0x35dce11b65edaa93,
    ),
    ("mutant", Recipe { wobble_hz: 2.0, wobble_cents: 300.0, chorus: 0.3, ..Recipe::DRY }, 0xde9c0ee7f82d025b),
    (
        "8bit",
        Recipe { crush_hz: 8000.0, crush_bits: 5, ring_hz: 440.0, ring_mix: 0.1, gain: 1.2, ..Recipe::DRY },
        0x1d0ffb0f6260a173,
    ),
    (
        "synth_choir",
        Recipe {
            vocoder_hz: 220.0,
            vocoder_chord: true,
            reverb_size: 0.6,
            reverb_damp: 0.4,
            reverb_mix: 0.2,
            gain: 1.4,
            ..Recipe::DRY
        },
        0x4fd7cb4c0fb52872,
    ),
    (
        "hologram",
        Recipe {
            ring_hz: 8.0,
            ring_mix: 0.25,
            flanger_hz: 0.6,
            flanger_mix: 0.5,
            echo_ms: 90.0,
            echo_feedback: 0.3,
            echo_mix: 0.25,
            ..Recipe::DRY
        },
        0x6776c3e323ba7544,
    ),
    (
        "spaceship",
        Recipe {
            highpass_hz: 500.0,
            lowpass_hz: 2500.0,
            hiss: 0.02,
            drive: 2.0,
            comb_ms: 1.5,
            comb_feedback: 0.3,
            ..Recipe::DRY
        },
        0x9af72353d14e52e2,
    ),
    (
        "broken_robot",
        Recipe {
            ring_hz: 55.0,
            ring_mix: 1.0,
            comb_ms: 6.0,
            comb_feedback: 0.55,
            wobble_hz: 6.0,
            wobble_cents: 250.0,
            dropout: 1.0,
            gain: 1.4,
            ..Recipe::DRY
        },
        0xcf2f767c59113cd4,
    ),
    (
        "helmet",
        Recipe {
            comb_ms: 0.8,
            comb_feedback: 0.5,
            eq_hz: 300.0,
            eq_db: 4.0,
            squash: 4.0,
            lowpass_hz: 5000.0,
            gain: 2.4,
            ..Recipe::DRY
        },
        0xe47f8402697369a6,
    ),
    ("cave", Recipe { reverb_size: 0.95, reverb_damp: 0.45, reverb_mix: 0.5, ..Recipe::DRY }, 0x7935b3e6e74924e4),
    ("cathedral", Recipe { reverb_size: 1.0, reverb_damp: 0.15, reverb_mix: 0.45, ..Recipe::DRY }, 0x24bf15ab7eac6862),
    ("bathroom", Recipe { reverb_size: 0.3, reverb_damp: 0.05, reverb_mix: 0.4, ..Recipe::DRY }, 0xdf36561186c35ac4),
    (
        "stadium",
        Recipe {
            echo_ms: 300.0,
            echo_feedback: 0.25,
            echo_mix: 0.35,
            reverb_size: 0.8,
            reverb_damp: 0.3,
            reverb_mix: 0.25,
            ..Recipe::DRY
        },
        0x0061a2d4959ad8c3,
    ),
    (
        "underwater",
        Recipe { lowpass_hz: 900.0, chorus: 0.5, vibrato_hz: 3.0, vibrato_cents: 25.0, gain: 1.5, ..Recipe::DRY },
        0xfc78fba1add84b4c,
    ),
    ("canyon", Recipe { echo_ms: 380.0, echo_feedback: 0.28, echo_mix: 0.5, ..Recipe::DRY }, 0xd0aeb1c2ed54cedc),
    ("tin_can", Recipe { comb_ms: 1.2, comb_feedback: 0.7, highpass_hz: 300.0, ..Recipe::DRY }, 0xb173eb6b1d66952d),
    (
        "phone",
        Recipe { highpass_hz: 300.0, lowpass_hz: 3400.0, drive: 1.5, gain: 1.3, ..Recipe::DRY },
        0x1f4526734968b9dd,
    ),
    (
        "megaphone",
        Recipe {
            highpass_hz: 500.0,
            lowpass_hz: 4000.0,
            drive: 5.0,
            hard_clip: true,
            reverb_size: 0.4,
            reverb_damp: 0.4,
            reverb_mix: 0.15,
            ..Recipe::DRY
        },
        0x51c225ef9267df79,
    ),
    (
        "walkie",
        Recipe {
            highpass_hz: 400.0,
            lowpass_hz: 3000.0,
            crush_hz: 8000.0,
            crush_bits: 8,
            hiss: 0.04,
            drive: 2.0,
            ..Recipe::DRY
        },
        0x51fc2a969e5e5ce1,
    ),
    (
        "old_radio",
        Recipe {
            highpass_hz: 300.0,
            lowpass_hz: 3500.0,
            drive: 2.5,
            hiss: 0.02,
            tremolo_hz: 0.5,
            tremolo_depth: 0.2,
            ..Recipe::DRY
        },
        0xbf02a0d94507ae28,
    ),
    (
        "drive_thru",
        Recipe { highpass_hz: 600.0, lowpass_hz: 3000.0, drive: 4.0, hiss: 0.03, ..Recipe::DRY },
        0xa9c9926f90baa077,
    ),
    (
        "cassette",
        Recipe { wobble_hz: 0.8, wobble_cents: 20.0, lowpass_hz: 7000.0, hiss: 0.01, drive: 1.3, ..Recipe::DRY },
        0x92a3d834476fcd3c,
    ),
    (
        "squad_radio",
        Recipe {
            highpass_hz: 400.0,
            lowpass_hz: 3000.0,
            squash: 4.0,
            drive: 2.0,
            hiss: 0.03,
            roger: 1.0,
            gain: 1.4,
            ..Recipe::DRY
        },
        0x214dd946c8a3aaae,
    ),
    (
        "cheap_headset",
        Recipe {
            highpass_hz: 250.0,
            lowpass_hz: 3200.0,
            drive: 6.0,
            hard_clip: true,
            crush_hz: 9000.0,
            crush_bits: 7,
            gain: 0.45,
            ..Recipe::DRY
        },
        0x15e0883c3c6c3003,
    ),
    (
        "bad_signal",
        Recipe {
            highpass_hz: 300.0,
            lowpass_hz: 3400.0,
            drive: 1.5,
            hiss: 0.02,
            dropout: 0.8,
            gain: 1.3,
            ..Recipe::DRY
        },
        0xca0dd1ee60c5205e,
    ),
    (
        "chipmunk_choir",
        Recipe { pitch: 12.0, harmony: [16.0, 19.0], harmony_mix: 0.5, ..Recipe::DRY },
        0x51f384cc60ed952a,
    ),
    (
        "opera",
        Recipe {
            vibrato_hz: 5.5,
            vibrato_cents: 45.0,
            reverb_size: 0.9,
            reverb_damp: 0.3,
            reverb_mix: 0.3,
            ..Recipe::DRY
        },
        0x03cb597bbc84d95c,
    ),
    (
        "choir",
        Recipe {
            harmony: [4.0, 7.0],
            harmony_mix: 0.5,
            chorus: 0.3,
            reverb_size: 0.9,
            reverb_damp: 0.3,
            reverb_mix: 0.3,
            ..Recipe::DRY
        },
        0x813fbb0ce1568ee9,
    ),
    (
        "karaoke",
        Recipe {
            echo_ms: 180.0,
            echo_feedback: 0.3,
            echo_mix: 0.3,
            reverb_size: 0.5,
            reverb_damp: 0.2,
            reverb_mix: 0.2,
            ..Recipe::DRY
        },
        0xb30226c100b91b9a,
    ),
    (
        "glitch",
        Recipe {
            wobble_hz: 8.0,
            wobble_cents: 60.0,
            crush_hz: 11000.0,
            crush_bits: 10,
            ring_hz: 15.0,
            ring_mix: 0.2,
            ..Recipe::DRY
        },
        0x639a104a89457e58,
    ),
    ("duet", Recipe { harmony: [-12.0, 0.0], harmony_mix: 0.5, ..Recipe::DRY }, 0x688d8b1cb33341ac),
    ("jet", Recipe { flanger_hz: 0.25, flanger_mix: 0.9, ..Recipe::DRY }, 0x9c0e4a57598df25b),
    (
        "helium_robot",
        Recipe { pitch: 9.0, ring_hz: 70.0, ring_mix: 0.7, comb_ms: 4.0, comb_feedback: 0.4, ..Recipe::DRY },
        0x41a910cf97c46967,
    ),
    (
        "slow_motion",
        Recipe { pitch: -10.0, formant: 0.8, reverb_size: 0.9, reverb_damp: 0.7, reverb_mix: 0.3, ..Recipe::DRY },
        0x51b64d60016ecfdc,
    ),
    ("autotune", Recipe { tune: 3, ..Recipe::DRY }, 0x7cfdf83209a90300),
    (
        "chord_choir",
        Recipe {
            tune: 2,
            harmony: [4.0, 7.0],
            harmony_mix: 0.6,
            reverb_size: 0.7,
            reverb_damp: 0.3,
            reverb_mix: 0.25,
            ..Recipe::DRY
        },
        0x6a4b893a1f9bd476,
    ),
    ("talkbox", Recipe { tune: 1, vocoder_hz: 110.0, ..Recipe::DRY }, 0xc60abda0d7acd4e9),
];

#[test]
fn every_voice_renders_the_golden_bits() {
    let mut changed = Vec::new();
    for (name, recipe, want) in CASES {
        let got = render(recipe);
        if got != *want {
            changed.push(format!("{name} {got:016x}"));
        }
    }
    assert!(changed.is_empty(), "voices whose output changed:\n{}", changed.join("\n"));
}
