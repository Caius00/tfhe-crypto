use std::mem::swap;
use std::time::Instant;
use axum_test::expect_json::__private::serde_trampoline::{Deserialize, Serialize};
use rayon::prelude::*;
use tfhe::FheUint8;
use tfhe::prelude::{FheOrd, FheTrivialEncrypt, IfThenElse};

#[derive(Serialize, Deserialize)]
pub struct EncryptedImage {
    pub(crate) pixels: Vec<FheUint8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl EncryptedImage {
    // PER_PIXEL
    fn base_func<F>(
        &mut self,
        function: F
    )
    where
        F: Fn(&FheUint8) -> FheUint8 + Sync + Send,
    {
        let now = Instant::now();
        let processed_img: Vec<FheUint8> = self.pixels
            .par_iter()
            .map(function)
            .collect();
        self.pixels = processed_img;

        let elapsed = now.elapsed();
        println!("Manipulation Finished in: {:?}", elapsed);
    }

    // Per Pixel extended by access to neighbors
    fn extended_base_func<F>(
        &mut self,
        function: F
    ) 
    where 
        F: Fn(u32, u32, &EncryptedImage) -> FheUint8 + Sync + Send,
    {
        let now = Instant::now();

        let width = self.width;
        let height = self.height;

        let processed: Vec<FheUint8> = (0..width * height)
            .into_par_iter()
            .map(|index| {
                let row = index / width;
                let col = index % width;

                function(row, col, self)
            })
            .collect();
        self.pixels = processed;

        println!(
            "Neighborhood manipulation finished in {:?}",
            now.elapsed()
        );
    }

    fn get_pixel(&self, row: u32, col: u32) -> &FheUint8 {
        let index = (row * self.width + col) as usize;
        &self.pixels[index]
    }

    pub fn invert(&mut self) {
        let inverting = |pixel_value: &FheUint8| !pixel_value;

        self.base_func(inverting);
    }

    pub fn white_threshold(&mut self) {
        let white_threshold = |pixel_value: &FheUint8| {
            pixel_value.gt(128).if_then_else(
                &FheUint8::encrypt_trivial(255u8),
                &pixel_value
            )
        };

        self.base_func(white_threshold);
    }

    pub fn black_threshold(&mut self) {
        let black_threshold = |pixel_value: &FheUint8| {
            pixel_value.lt(128).if_then_else(
                &FheUint8::encrypt_trivial(0u8),
                &pixel_value
            )
        };

        self.base_func(black_threshold);
    }

    pub fn blooming(&mut self) {
        self.extended_base_func(|row, col, image| {
            if row == 0 || col == 0 ||row == image.height - 1 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }

            let threshhold = 220;
            
            let center = image.get_pixel(row, col);
            
            let n  = image.get_pixel(row - 1, col);
            let s  = image.get_pixel(row + 1, col);
            let e  = image.get_pixel(row, col + 1);
            let w  = image.get_pixel(row, col - 1);
            
            let ne = image.get_pixel(row - 1, col + 1);
            let nw = image.get_pixel(row - 1, col - 1);
            let se = image.get_pixel(row + 1, col + 1);
            let sw = image.get_pixel(row + 1, col - 1);
            
            let strong = FheUint8::encrypt_trivial(20u8);
            let weak = FheUint8::encrypt_trivial(10u8);
            let zero = FheUint8::encrypt_trivial(0u8);

            let boost = n.gt(128).if_then_else(&strong, &zero)
            + s.gt(threshhold).if_then_else(&strong, &zero)
            + e.gt(threshhold).if_then_else(&strong, &zero)
            + w.gt(threshhold).if_then_else(&strong, &zero)
            + ne.gt(threshhold).if_then_else(&weak, &zero)
            + nw.gt(threshhold).if_then_else(&weak, &zero)
            + se.gt(threshhold).if_then_else(&weak, &zero)
            + sw.gt(threshhold).if_then_else(&weak, &zero);
            
            center + boost
    });}

    pub fn box_blur(&mut self) {
        self.extended_base_func(|row, col, image| {
            if row == 0 || col == 0 ||row == image.height - 1 || col == image.width - 1 {
                return image.get_pixel(row, col).clone();
            }
            
            let center = image.get_pixel(row, col);
            let north  = image.get_pixel(row - 1, col);
            let south  = image.get_pixel(row + 1, col);
            let east  = image.get_pixel(row, col + 1);
            let west  = image.get_pixel(row, col - 1);

            let sum = center + north + south + west + east;
            
            sum / 5u8
    });}

    // ROTATIONS:
    pub fn rotate_90(&mut self) {
        let now = Instant::now();

        if self.is_square() {
            self.transpose_square_matrix();
            self.flip_horizontal();
        } else {
            self.non_square_90_degrees()
        }
        let elapsed = now.elapsed();

        println!("90° Rotation Finished in: {:?}", elapsed);

    }

    pub fn rotate_180(&mut self) {
        let now = Instant::now();

        self.flip_horizontal();
        self.flip_vertical();

        let elapsed = now.elapsed();
        println!("180° Rotation Finished in: {:?}", elapsed);
    }

    pub fn rotate_270(&mut self) {
        let now = Instant::now();

        if self.is_square() {
            self.transpose_square_matrix();
            self.flip_vertical();
        } else {
            self.non_square_270_degrees()
        }
        let elapsed = now.elapsed();
        println!("270° Rotation Finished in: {:?}", elapsed);
    }

    fn transpose_square_matrix(&mut self) {
        let size = self.width;
        // Index entry by row * size + col
        // Only iterate "upper" triangle (with row+1..size)
        for row in 0..size {
            for column in row+1..size {
                let source_index = row * size + column;
                let destination_index = column * size + row;
                self.pixels.swap(source_index as usize, destination_index as usize);
            }
        }
    }

    /// TODO() Check if clone overhead is better
    fn non_square_90_degrees(&mut self) {
        let width = self.width;
        let height = self.height;
        // idea, create new array and pick every element
        // we do this to avoid cloning
        let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity((width * height) as usize);

        // loop through new vec so that values are stored next to each other in memory
        for new_row in 0..width {
            for new_column in 0..height {
                // calculate index where to take value from
                let old_row = height - 1 - new_column;
                let old_column = new_row;

                // height and width reverse due to rotation
                let old_index = (old_row * width + old_column) as usize;

                new_encrypted_image_data.push(self.pixels[old_index].clone())
            }
        }
        self.swap_width_height();
        self.pixels = new_encrypted_image_data;
    }

    fn non_square_270_degrees(&mut self) {
        let width = self.width;
        let height = self.height;
        // idea, create new array and pick every element
        // we do this to avoid cloning
        let mut new_encrypted_image_data: Vec<FheUint8> = Vec::with_capacity((width * height) as usize);

        // loop through new vec so that values are stored next to each other in memory
        for new_row in 0..width {
            for new_column in 0..height {
                // calculate index where to take value from
                let old_row = new_column;
                let old_column = new_row;

                // height and width reverse due to rotation
                let old_index = (old_row * width + old_column) as usize;

                new_encrypted_image_data.push(self.pixels[old_index].clone())
            }
        }
        self.swap_width_height();
        self.pixels = new_encrypted_image_data;
    }

    pub fn flip_horizontal(&mut self) {
        let now = Instant::now();

        for row in self.pixels.chunks_mut(self.width as usize) {
            row.reverse();
        }

        let elapsed = now.elapsed();
        println!("Horizontal-Flip Finished in: {:?}", elapsed);
    }

    pub fn flip_vertical(&mut self) {
        let now = Instant::now();

        // swaps top and bottom row and removes them, until all rows processed
        let mut data = &mut self.pixels[..];
        // while two rows remaining
        while data.len() >= (self.width * 2) as usize {
            let (top_row, rest) = data.split_at_mut(self.width as usize);
            let (middle, bottom_row) = rest.split_at_mut(rest.len() - self.width as usize);

            top_row.swap_with_slice(bottom_row);
            data = middle;
        }
        let elapsed = now.elapsed();
        println!("Vertical-Flip Finished in: {:?}", elapsed);
    }

    fn swap_width_height(&mut self) {
        swap(&mut self.width, &mut self.height)
    }

    fn is_square(&self) -> bool {
        self.width == self.height
    }
    fn is_large(&self) -> bool {
        self.width > 512 || self.height > 512
    }
}
