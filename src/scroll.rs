pub fn max_offset(total_rows: usize, viewport_rows: usize) -> usize {
    total_rows.saturating_sub(viewport_rows)
}

pub fn clamp_offset(offset: usize, total_rows: usize, viewport_rows: usize) -> usize {
    offset.min(max_offset(total_rows, viewport_rows))
}

pub fn scroll_by(offset: usize, delta: isize, total_rows: usize, viewport_rows: usize) -> usize {
    let next = if delta.is_negative() {
        offset.saturating_sub(delta.unsigned_abs())
    } else {
        offset.saturating_add(delta as usize)
    };
    clamp_offset(next, total_rows, viewport_rows)
}

pub fn page_step(viewport_rows: usize) -> usize {
    viewport_rows.saturating_sub(2).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_available_rows() {
        assert_eq!(clamp_offset(100, 20, 10), 10);
        assert_eq!(clamp_offset(100, 5, 10), 0);
    }

    #[test]
    fn scrolls_with_saturation() {
        assert_eq!(scroll_by(0, -4, 100, 20), 0);
        assert_eq!(scroll_by(0, 4, 100, 20), 4);
        assert_eq!(scroll_by(90, 20, 100, 20), 80);
    }

    #[test]
    fn page_step_leaves_overlap() {
        assert_eq!(page_step(20), 18);
        assert_eq!(page_step(1), 1);
    }
}
