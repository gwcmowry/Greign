//! Workgroup dispatch geometry.

/// Bounded two-dimensional compute dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchGeometry {
    /// Source pixel width.
    pub width: u32,
    /// Source pixel height.
    pub height: u32,
    /// Shader workgroup width.
    pub workgroup_width: u32,
    /// Shader workgroup height.
    pub workgroup_height: u32,
    /// Workgroups dispatched on X.
    pub groups_x: u32,
    /// Workgroups dispatched on Y.
    pub groups_y: u32,
}

impl DispatchGeometry {
    /// Computes ceil-divided dispatch counts.
    ///
    /// Returns `None` for an empty image or workgroup.
    #[must_use]
    pub fn new(
        width: u32,
        height: u32,
        workgroup_width: u32,
        workgroup_height: u32,
    ) -> Option<Self> {
        if width == 0 || height == 0 || workgroup_width == 0 || workgroup_height == 0 {
            return None;
        }
        Some(Self {
            width,
            height,
            workgroup_width,
            workgroup_height,
            groups_x: width.div_ceil(workgroup_width),
            groups_y: height.div_ceil(workgroup_height),
        })
    }

    /// Total shader invocations, including guarded boundary lanes.
    #[must_use]
    pub fn invocation_count(self) -> u64 {
        u64::from(self.groups_x)
            * u64::from(self.groups_y)
            * u64::from(self.workgroup_width)
            * u64::from(self.workgroup_height)
    }
}

#[cfg(test)]
mod tests {
    use super::DispatchGeometry;

    #[test]
    fn non_multiple_dimensions_round_up() {
        let geometry = DispatchGeometry::new(17, 33, 16, 16).expect("geometry");
        assert_eq!((geometry.groups_x, geometry.groups_y), (2, 3));
        assert_eq!(geometry.invocation_count(), 1_536);
    }

    #[test]
    fn single_pixel_dispatches_one_group() {
        let geometry = DispatchGeometry::new(1, 1, 16, 16).expect("geometry");
        assert_eq!((geometry.groups_x, geometry.groups_y), (1, 1));
    }

    #[test]
    fn empty_dimensions_do_not_dispatch() {
        assert!(DispatchGeometry::new(0, 10, 16, 16).is_none());
        assert!(DispatchGeometry::new(10, 0, 16, 16).is_none());
    }
}
