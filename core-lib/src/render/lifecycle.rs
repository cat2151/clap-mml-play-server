//! [`RealtimeRenderer`] の CLAP processor 終了処理。

use super::RealtimeRenderer;

impl Drop for RealtimeRenderer {
    fn drop(&mut self) {
        let Some(processor) = self.processor.take() else {
            return;
        };
        let Some(mut plugin_instance) = self.plugin_instance.take() else {
            return;
        };
        let stopped = processor.stop_processing();
        plugin_instance.deactivate(stopped);
    }
}
