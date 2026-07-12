use crate::AssetProcessor;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use wesl::syntax::PathOrigin;
use wesl::{ModulePath, Wesl};

pub struct WeslProcessor;

impl AssetProcessor for WeslProcessor {
    type Options = ();

    fn process(&self, _: &[u8], src_path: &Path, mut out: File, _: Self::Options) {
        let out_string = Wesl::new(src_path.parent().unwrap())
            .compile(&ModulePath::new(
                PathOrigin::Absolute,
                vec![src_path.file_name().unwrap().to_str().unwrap().to_owned()],
            ))
            .inspect_err(|e| eprintln!("WESL error: {e}"))
            .unwrap()
            .to_string();
        out.write_all(out_string.as_bytes()).unwrap();
    }
}
