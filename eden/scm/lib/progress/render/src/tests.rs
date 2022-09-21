/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use progress_model::CacheStats;
use progress_model::IoTimeSeries;
use progress_model::ProgressBar;
use progress_model::Registry;

use crate::RenderingConfig;

#[test]
fn test_simple_render() {
    let reg = example();
    let mut config = RenderingConfig::for_testing();
    assert_eq!(
        format!("\r\n{}", crate::simple::render_string(&reg, &config)),
        r#"
       Files  110 (9% miss)
       Trees  110 (9% miss)
         Net  [ ▁▁▂▂▃▃▄▄▅▅▆▆▇█]  ▼ 67KB/s  154 requests  t…
        Disk  [ ▁▁▂▂▃▃▄▄▅▅▆▆▇█]  ▲ 4050B/s  total 58KB up
       Files  [=======>       ]  5KB/10KB
       Trees  [     <=>       ]  5KB
  Defragging  [=======>       ]  5KB/10KB
       Files  [=======>       ]  5KB/10KB  ./foo/Files/文…
       Trees  [     <=>       ]  5KB  ./foo/Trees/文件名
              and 4 more"#
            .replace('\n', "\r\n")
    );

    config.term_width = 80;
    assert_eq!(
        format!("\r\n{}", crate::simple::render_string(&reg, &config)),
        r#"
           Files  110 (9% miss)
           Trees  110 (9% miss)
             Net  [ ▁▁▂▂▃▃▄▄▅▅▆▆▇█]  ▼ 67KB/s  154 requests  total 980KB down
            Disk  [ ▁▁▂▂▃▃▄▄▅▅▆▆▇█]  ▲ 4050B/s  total 58KB up
           Files  [=======>       ]  5KB/10KB
           Trees  [     <=>       ]  5KB
Defragging disks  [=======>       ]  5KB/10KB
           Files  [=======>       ]  5KB/10KB  ./foo/Files/文件名
           Trees  [     <=>       ]  5KB  ./foo/Trees/文件名
                  and 4 more"#
            .replace('\n', "\r\n")
    );
}

/// Example registry with some progress bars.
fn example() -> Registry {
    let reg = Registry::default();

    // Time series.
    for &(topic, unit) in &[("Net", "requests"), ("Disk", "files")] {
        let series = IoTimeSeries::new(topic, unit);
        if topic == "Net" {
            series.populate_test_samples(1, 0, 11);
        } else {
            series.populate_test_samples(0, 1, 0);
        }
        reg.register_io_time_series(&series);
    }

    // Cache stats
    for &topic in &["Files", "Trees"] {
        let stats = CacheStats::new(topic);
        stats.increase_hit(100);
        stats.increase_miss(10);
        reg.register_cache_stats(&stats);
    }

    // Progress bars
    for i in 0..3 {
        for &topic in &["Files", "Trees", "defragging disks"] {
            let total = if topic == "Trees" { 0 } else { 10000 };
            let bar = ProgressBar::new(topic, total, "bytes");
            bar.increase_position(5000);
            reg.register_progress_bar(&bar);
            if i == 1 {
                let message = format!("./foo/{}/文件名", topic);
                bar.set_message(message);
            }
        }
    }

    reg
}
