//! 页面状态回归测试: 使用实际 Cx 与内置字体, 不访问设备或网络。
use super::*;

pub fn with_cx(f: impl FnMut(&mut kn_ui::Cx<KinNovel>, &mut kn_render::Bitmap) + 'static) {
    struct Harness<F>(F);
    impl<F: FnMut(&mut kn_ui::Cx<KinNovel>, &mut kn_render::Bitmap)> Page<KinNovel> for Harness<F> {
        fn id(&self) -> kn_ui::PageId { kn_ui::PageId(9999) }
        fn render(&mut self, cx: &mut kn_ui::Cx<KinNovel>, frame: &mut kn_render::Bitmap) { (self.0)(cx, frame); }
        fn on_input(&mut self, _: &mut kn_ui::Cx<KinNovel>, _: &InputEvent) -> kn_ui::Transition<KinNovel> { kn_ui::Transition::None }
    }
    let dir = std::env::temp_dir().join(format!("kn-page-test-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let paths = Paths { app_dir: dir.clone() };
    let config = Config::load(paths.settings_file());
    let mut fonts = FontStore::new();
    let chain = vec![fonts.load_bytes(SYMBOL_FONT.to_vec()).unwrap()];
    let app = KinNovel::new(paths, config, chain, 1236, 1648, 300, kn_platform::Frontlight::detect(None), false);
    let mut host = kn_ui::Headless::new(app, fonts, 1236, 1648).unwrap();
    host.push(Box::new(Harness(f)));
    host.frame();
    host.app().net_client.shutdown();
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}
