/* Benchmarks for fdtables.  This does a few basic operations related to
 * virtual fd -> real fd translation */

use criterion::{criterion_group, criterion_main, Criterion};

use fdtables::*;

use std::time::Duration;

pub fn run_benchmark(c: &mut Criterion) {

    // I'm mostly going to do a ton of translate_virtual_fd calls.  I'll do
    // a little bit of setup with get_unused_virtual_fd first...
    let mut group = c.benchmark_group("fdtables basics");


    // Reduce the time to reduce disk space needed and go faster.
    // Default is 5s...
    group.measurement_time(Duration::from_secs(2));

    // Shorten the warm up time as well from 3s to this...
    group.warm_up_time(Duration::from_secs(1));


    // I'm going to insert three items (all with cloexec), then do 10000 
    // queries, then call the helper function to cloexec and clean up...
    group.bench_function("translate_virtual_fd (10000)",
            |b| b.iter(|| {
                let fd1= get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
                let fd2= get_unused_virtual_fd(threei::TESTING_CAGEID, 20, true, 1).unwrap();
                let fd3= get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
                for _ in [0..1000].iter() {
                    translate_virtual_fd(threei::TESTING_CAGEID, fd1).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd1).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd1).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                    translate_virtual_fd(threei::TESTING_CAGEID, fd1).unwrap();
                }
                _ = empty_fds_for_exec(threei::TESTING_CAGEID);
            })
        );
    group.finish();
}


criterion_group!(benches, run_benchmark);
criterion_main!(benches);
