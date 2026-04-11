//! `egui-text-animation` provides simple text animation utilities for the [egui](https://github.com/emilk/egui) library.
//!
//! This library offers the [`TextAnimator`] struct to create various text animations. It's designed to be
//! easy to integrate into your existing egui applications.  The core of the library is the
//! [`TextAnimator`] struct, which manages the state of the animation, and the [`AnimationType`] enum,
//! which determines the type of animation to perform.
//!
//! # Features
//!
//! *   **Fade-in Animation:** Animate text to gradually appear, character by character.  See [`AnimationType::FadeIn`].
//! *   **Typewriter Animation:** Animate text to appear as if it's being typed.  See [`AnimationType::Typewriter`].
//! *   **Hacker Animation:** Animate text to appear as if it's being decoded.  See [`AnimationType::Hacker`].
//! *   **Customizable Speed:** Control the speed of the animation with [`TextAnimator::set_speed`].
//! *   **Easy Integration:** Simply create a [`TextAnimator`], call [`TextAnimator::process_animation`] each frame,
//!     and then render with [`TextAnimator::render`].
//! *   **Automatic Repainting:** Call `ctx.request_repaint()` inside your update loop to ensure smooth animation.
//! *   **Animation Control:**  You can check if the animation is finished with [`TextAnimator::is_animation_finished`] and reset it with [`TextAnimator::reset`].
//!
//!
//! # Animation Types
//!
//! The [`AnimationType`] enum provides the following animation types:
//!
//! *   [`AnimationType::FadeIn`]:  Characters gradually fade in from transparent to fully opaque.
//! *   [`AnimationType::Typewriter`]: Characters appear one by one, simulating a typewriter effect.
//! *   [`AnimationType::Hacker`]: Characters cycle through random characters before settling on the final character.
//!
//! # Notes
//!
//! *   The `unstable_dt` value from `ctx.input(|i| i.unstable_dt)` is used for frame-independent
//!     animation timing.  This ensures the animation runs at the correct speed regardless of
//!     the application's frame rate.
//! *   It's crucial to call `ctx.request_repaint()` during the animation to ensure that
//!     egui re-renders the UI, thus updating the animation.

use eframe::epaint::text::{LayoutJob, TextFormat};
use eframe::epaint::{Color32, FontFamily, FontId};
use rand::prelude::*;

/// Enum representing the available animation types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationType {
    /// Characters gradually fade in from transparent to fully opaque.
    FadeIn,
    /// Characters appear one by one, simulating a typewriter effect.
    Typewriter,
    /// Characters cycle through random characters before settling on the final character.
    Hacker,
}

/// A struct for creating and managing text animations.
#[derive(Clone, Debug)]
pub struct TextAnimator {
    pub text: String,
    pub font: FontId,
    pub color: Color32,
    pub timer: f32,
    pub speed: f32,
    pub animation_finished: bool,
    pub animation_type: AnimationType,
    intermediate_text: Vec<char>,
}

impl Default for TextAnimator {
    fn default() -> Self {
        Self {
            text: "Hello, World!".to_string(),
            font: FontId::new(12.0, FontFamily::Proportional),
            color: Color32::WHITE,
            timer: 0.0,
            speed: 2.5,
            animation_finished: false,
            animation_type: AnimationType::FadeIn,
            intermediate_text: Vec::new(),
        }
    }
}

impl TextAnimator {
    /// Creates a new `TextAnimator` with the given text, font, color, speed, and animation type.
    ///
    /// # Arguments
    ///
    /// * `text`: The text to animate.
    /// * `font`: The font to use for the text.
    /// * `color`: The color of the text.
    /// * `speed`: The speed of the animation.  Higher values mean faster animation.
    /// * `animation_type`: The type of animation to use.
    pub fn new(
        text: &str,
        font: FontId,
        color: Color32,
        speed: f32,
        animation_type: AnimationType,
    ) -> Self {
        let intermediate_text = vec![' '; text.len()]; // Initialize with spaces
        Self {
            text: text.to_string(),
            font,
            color,
            timer: 0.0,
            speed,
            animation_finished: false,
            animation_type,
            intermediate_text,
        }
    }

    /// Sets the animation speed.
    ///
    /// # Arguments
    ///
    /// * `speed`: The new speed of the animation. Higher values mean faster animation.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// Resets the animation timer to the beginning, effectively restarting the animation.
    pub fn reset(&mut self) {
        self.timer = 0.0;
        self.animation_finished = false;
        if self.animation_type == AnimationType::Hacker {
            self.intermediate_text = vec![' '; self.text.len()];
        }
    }

    /// Processes the animation, updating the internal timer based on the elapsed time
    /// and the animation type.  This should be called every frame.
    ///
    /// # Arguments
    ///
    /// * `ctx`: The `egui::Context` to access time information.
    pub fn process_animation(&mut self, ctx: &egui::Context) {
        if self.animation_finished {
            return;
        }

        let dt = ctx.input(|i| i.unstable_dt);
        let increment = dt * self.speed;

        if self.animation_type == AnimationType::Hacker {
            let chars: Vec<char> = self.text.chars().collect();
            let num_chars = chars.len();
            let visible_chars = (self.timer * num_chars as f32).floor() as usize;

            let mut rng = rand::rng();

            for i in 0..visible_chars.min(num_chars) {
                if self.intermediate_text.get(i) != chars.get(i) {
                    let random_char = rng.random_range(33..=126) as u8 as char;
                    if let Some(intermediate_char) = self.intermediate_text.get_mut(i) {
                        *intermediate_char = random_char;
                    }
                }
            }

            let lock_threshold = 0.1;

            for i in 0..visible_chars.min(num_chars) {
                if self.intermediate_text.get(i) != chars.get(i)
                    && rng.random_range(0.0..1.0) < lock_threshold
                {
                    if let Some(intermediate_char) = self.intermediate_text.get_mut(i) {
                        *intermediate_char = chars[i];
                    }
                }
            }

            self.timer = (self.timer + increment).min(num_chars as f32);
            if self.timer >= num_chars as f32 {
                self.animation_finished = true;
            }
        } else {
            self.timer = (self.timer + increment).min(1.0);
            if self.timer >= 1.0 {
                self.animation_finished = true;
            }
        }
    }

    /// Returns `true` if the animation has finished, `false` otherwise.
    pub fn is_animation_finished(&self) -> bool {
        self.animation_finished
    }

    /// Renders the text animation within the given UI, based on the animation type.
    /// This function handles selecting the correct rendering method based on `animation_type`.
    ///
    /// # Arguments
    ///
    /// * `ui`: The `egui::Ui` instance to render the animation into.
    pub fn render(&mut self, ui: &mut egui::Ui) {
        match self.animation_type {
            AnimationType::FadeIn => self.fade_in_text(ui),
            AnimationType::Typewriter => self.typewriter_text(ui),
            AnimationType::Hacker => self.hacker_text(ui),
        }
    }

    /// Renders the fade-in text animation.  Characters are rendered with increasing opacity
    /// based on the animation timer.
    fn fade_in_text(&self, ui: &mut egui::Ui) {
        let chars: Vec<char> = self.text.chars().collect();
        let num_chars = chars.len();
        let visible_chars_float = self.timer * num_chars as f32;
        let visible_chars = visible_chars_float.floor() as usize;
        let remainder = visible_chars_float - visible_chars_float.floor();

        let mut job = LayoutJob::default();
        for (i, ch) in chars.iter().enumerate() {
            let char_alpha_f32 = if i < visible_chars {
                1.0
            } else if i == visible_chars && i < num_chars {
                remainder
            } else {
                0.0
            };
            job.append(
                &ch.to_string(),
                0.0,
                TextFormat {
                    color: self.color.gamma_multiply(char_alpha_f32),
                    font_id: self.font.clone(),
                    ..Default::default()
                },
            );
        }
        ui.label(job);
    }

    /// Renders the typewriter text animation.  Characters are rendered one by one based
    /// on the animation timer.
    fn typewriter_text(&self, ui: &mut egui::Ui) {
        let chars: Vec<char> = self.text.chars().collect();
        let num_chars = chars.len();
        let visible_chars = (self.timer * num_chars as f32).floor() as usize;

        let mut job = LayoutJob::default();
        for (i, ch) in chars.iter().enumerate() {
            if i < visible_chars {
                job.append(
                    &ch.to_string(),
                    0.0,
                    TextFormat {
                        color: self.color,
                        font_id: self.font.clone(),
                        ..Default::default()
                    },
                );
            } // No else clause needed - we simply don't add invisible characters
        }
        ui.label(job);
    }

    /// Renders the hacker text animation. Characters are rendered one by one, first showing
    /// random characters and then the final character.
    fn hacker_text(&self, ui: &mut egui::Ui) {
        let mut job = LayoutJob::default();
        for (i, &ch) in self.intermediate_text.iter().enumerate() {
            // Check if we've reached the final character, and display it.
            if self.text.chars().nth(i) == Some(ch) {
                job.append(
                    &ch.to_string(),
                    0.0,
                    TextFormat {
                        color: self.color,
                        font_id: self.font.clone(),
                        ..Default::default()
                    },
                );
            } else {
                // Display the intermediate (random) character.
                job.append(
                    &ch.to_string(),
                    0.0,
                    TextFormat {
                        color: self.color, // Could make this a different color for "hacker" effect
                        font_id: self.font.clone(),
                        ..Default::default()
                    },
                );
            }
        }
        ui.label(job);
    }
}
