/* Benchmarks for fdtables.  This does a few basic operations related to
 * virtual fd -> real fd translation */

use criterion::{criterion_group, criterion_main, Criterion};

use fdtables::*;

//use std::time::Duration;

pub fn run_benchmark(c: &mut Criterion) {
    // I'm going to do some simple calls using fdtables in this file
    let mut group = c.benchmark_group("fdtables basics");

    // Reduce the time to reduce disk space needed and go faster.
    // Default is 5s...
    //group.measurement_time(Duration::from_secs(2));

    // Shorten the warm up time as well from 3s to this...
    //group.warm_up_time(Duration::from_secs(1));

    let fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
    let fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 20, true, 1).unwrap();
    let fd3 = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();

    // I'm going to insert three items, then do 10000 queries, then clean up...
    group.bench_function("translate_virtual_fd (10000)", |b| {
        b.iter(|| {
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
        })
    });

    _flush_fdtable();

    // only do 1000 because 1024 is a common lower bound
    group.bench_function("get_unused_virtual_fd (1000)", |b| {
        b.iter(|| {
            for _ in [0..1000].iter() {
                _ = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
            }
            // unfortunately, we need to clean up, or else we will
            // get an exception due to the table being full...
            _flush_fdtable();
        })
    });

    // Check get_optionalinfo...
    let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
    group.bench_function("get_optionalinfo (10000)", |b| {
        b.iter(|| {
            for _ in [0..10000].iter() {
                _ = get_optionalinfo(threei::TESTING_CAGEID, fd).unwrap();
            }
        })
    });

    _flush_fdtable();

    // flip the set_optionalinfo data...
    let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
    group.bench_function("set_optionalinfo (10000)", |b| {
        b.iter(|| {
            for _ in [0..5000].iter() {
                _ = set_optionalinfo(threei::TESTING_CAGEID, fd, 100).unwrap();
                _ = set_optionalinfo(threei::TESTING_CAGEID, fd, 200).unwrap();
            }
        })
    });

    _flush_fdtable();

    group.finish();
}

criterion_group!(benches, run_benchmark);
criterion_main!(benches);
