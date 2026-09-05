//! Frame-level parallelism with nothing but `std`.
//!
//! The image is cut into horizontal strips and workers pull the next strip from a
//! shared queue. Dynamic hand-out matters here: a strip of empty sky costs a
//! fraction of a strip crossing the island, so a fixed split would leave threads
//! idle waiting for the slowest one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

pub fn thread_count() -> usize {
    thread::available_parallelism().map_or(1, |n| n.get())
}

/// Run `render(strip, first_row)` over every strip of `rows_per_strip` rows.
///
/// `pixels` is the whole framebuffer; each strip is a disjoint `&mut` slice of it,
/// so workers never touch the same pixel and no locking is needed while shading.
pub fn render_strips<F>(pixels: &mut [u32], width: usize, rows_per_strip: usize, threads: usize, render: F)
where
    F: Fn(&mut [u32], usize) + Sync,
{
    let rows_per_strip = rows_per_strip.max(1);
    let mut strips: Vec<(usize, &mut [u32])> = pixels
        .chunks_mut(width * rows_per_strip)
        .enumerate()
        .map(|(i, slice)| (i * rows_per_strip, slice))
        .collect();

    if threads <= 1 || strips.len() <= 1 {
        for (first_row, slice) in strips.iter_mut() {
            render(slice, *first_row);
        }
        return;
    }

    let next = AtomicUsize::new(0);
    let queue = Mutex::new(strips);
    let render = &render;

    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                // The lock is held only long enough to claim an index.
                let index = next.fetch_add(1, Ordering::Relaxed);
                let claimed = {
                    let mut guard = queue.lock().unwrap();
                    if index >= guard.len() {
                        None
                    } else {
                        // Swap the slice out so the borrow leaves the mutex with us.
                        let (first_row, slice) = &mut guard[index];
                        Some((*first_row, std::mem::take(slice)))
                    }
                };
                match claimed {
                    Some((first_row, slice)) => render(slice, first_row),
                    None => break,
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(threads: usize, rows_per_strip: usize) -> Vec<u32> {
        let (width, height) = (37usize, 91usize);
        let mut pixels = vec![0u32; width * height];
        render_strips(&mut pixels, width, rows_per_strip, threads, |strip, first_row| {
            for (i, px) in strip.iter_mut().enumerate() {
                let y = first_row + i / width;
                let x = i % width;
                *px = (y * width + x) as u32;
            }
        });
        pixels
    }

    #[test]
    fn every_pixel_is_written_exactly_once() {
        let expected: Vec<u32> = (0..37 * 91).collect();
        assert_eq!(fill(1, 4), expected);
    }

    #[test]
    fn threaded_output_matches_single_threaded_output() {
        let single = fill(1, 4);
        for threads in [2, 3, 8, 16] {
            for rows in [1, 4, 16, 1000] {
                assert_eq!(fill(threads, rows), single, "threads={threads} rows={rows}");
            }
        }
    }

    #[test]
    fn thread_count_is_at_least_one() {
        assert!(thread_count() >= 1);
    }
}
