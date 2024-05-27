/* Benchmarks for fdtables.  This does a few basic operations related to
 * virtual fd -> real fd translation */

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use fdtables::*;

use std::thread;

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
    group.bench_function(format!("{}/st: trans (10K)", ALGONAME), |b| {
        b.iter(|| {
            for _ in 0..1000 {
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

    refresh();

    // only do 1000 because 1024 is a common lower bound
    group.bench_function(format!("{}/st: getvirt (1K)", ALGONAME), |b| {
        b.iter(|| {
            for _ in 0..1000 {
                _ = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
            }
            // unfortunately, we need to clean up, or else we will
            // get an exception due to the table being full...
            refresh();
        })
    });

    // Check get_optionalinfo...
    let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
    group.bench_function(format!("{}/st: get_opt (10K)", ALGONAME), |b| {
        b.iter(|| {
            for _ in 0..10000 {
                _ = get_optionalinfo(threei::TESTING_CAGEID, fd).unwrap();
            }
        })
    });

    refresh();

    // flip the set_optionalinfo data...
    let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 10).unwrap();
    group.bench_function(format!("{}/st: set_opt (10K)", ALGONAME), |b| {
        b.iter(|| {
            for _ in 0..5000 {
                _ = set_optionalinfo(threei::TESTING_CAGEID, fd, 100).unwrap();
                _ = set_optionalinfo(threei::TESTING_CAGEID, fd, 200).unwrap();
            }
        })
    });

    refresh();

    // ---------------- MULTI-THREADED TESTS ------------------  //

    // -- Multithreaded benchmark 1: 100K translate calls --

    let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
    let fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 20, true, 200).unwrap();
    let fd3 = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 300).unwrap();

    for threadcount in [1, 2, 4, 8, 16].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                format!("{}/[mt:{}] trans_virtfd (100K)", ALGONAME, threadcount),
                threadcount,
            ),
            threadcount,
            |b, threadcount| {
                b.iter({
                    || {
                        let mut thread_handle_vec: Vec<thread::JoinHandle<()>> = Vec::new();
                        for _numthreads in 0..*threadcount {
                            // Need to borrow so the lifetime can live outside
                            // the thread's closure
                            let thisthreadcount = *threadcount;

                            thread_handle_vec.push(thread::spawn(move || {
                                // Do 10K / threadcount of 10 requests each.  100K total
                                for _ in 0..10000 / thisthreadcount {
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                                }
                            }));
                        }
                        for handle in thread_handle_vec {
                            handle.join().unwrap();
                        }
                    }
                })
            },
        );
    }
    refresh();

    // -- Multithreaded benchmark 2: get / translate interleaved --

    // I will always do 100K requests (split amongst some number of threads)

    for threadcount in [1, 2, 4, 8, 16].iter() {
        group.bench_with_input(
            BenchmarkId::new(
                format!("{}/[mt:{}] get_trans (1K per)", ALGONAME, threadcount),
                threadcount,
            ),
            threadcount,
            |b, threadcount| {
                b.iter({
                    || {
                        let mut thread_handle_vec: Vec<thread::JoinHandle<()>> = Vec::new();
                        for _numthreads in 0..*threadcount {
                            // Need to borrow so the lifetime can live outside
                            // the thread's closure
                            let thisthreadcount = *threadcount;

                            thread_handle_vec.push(thread::spawn(move || {
                                // Do 1K / threadcount of 10 requests each.  100K total
                                for _ in 0..1000 / thisthreadcount {
                                    let fd = get_unused_virtual_fd(
                                        threei::TESTING_CAGEID,
                                        10,
                                        true,
                                        100,
                                    )
                                    .unwrap();
                                    translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                                }
                            }));
                        }
                        for handle in thread_handle_vec {
                            handle.join().unwrap();
                        }
                        refresh();
                    }
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, run_benchmark);
criterion_main!(benches);
