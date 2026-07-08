use crate::{AssetProcessor, AssetProcessorContext};
use std::io::Write;
use wesl::syntax::PathOrigin;
use wesl::{ModulePath, Wesl};

pub struct WeslProcessor;

impl AssetProcessor for WeslProcessor {
    type Options = ();

    fn process(&self, ctx: AssetProcessorContext, _: Self::Options) {
        let out_string = Wesl::new(ctx.src_path.parent().unwrap())
            .compile(&ModulePath::new(
                PathOrigin::Absolute,
                vec![
                    ctx.src_path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                ],
            ))
            .inspect_err(|e| eprintln!("WESL error: {e}"))
            .unwrap()
            .to_string();
        ctx.emit_main().write_all(out_string.as_bytes()).unwrap();
    }
}
