use std::path::{Path, PathBuf};

pub fn is_path_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    if roots.is_empty() {
        return false;
    }

    let Ok(canonical_path) = path.canonicalize() else {
        return false;
    };

    roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| canonical_path.starts_with(canonical_root))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::is_path_within_roots;
    use std::fs;
    use std::path::PathBuf;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-{name}-{nanos}"));
        dir
    }

    #[test]
    fn accepts_path_under_root() {
        let root = unique_temp_dir("root-ok");
        let nested = root.join("movies");
        fs::create_dir_all(&nested).expect("create nested dir");

        let media = nested.join("file.mkv");
        fs::write(&media, b"x").expect("write media");

        assert!(is_path_within_roots(&media, std::slice::from_ref(&root)));

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[test]
    fn rejects_path_outside_root() {
        let root = unique_temp_dir("root-no");
        let outside = unique_temp_dir("outside-no");
        fs::create_dir_all(&root).expect("create root dir");
        fs::create_dir_all(&outside).expect("create outside dir");

        let media = outside.join("file.mkv");
        fs::write(&media, b"x").expect("write outside media");

        assert!(!is_path_within_roots(&media, &[root.clone()]));

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
