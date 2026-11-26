//! Checks the values from two different loop back edges through the normal DXIL writer.
use super::bitcode::{Attr, Bin, Term, pred};
use super::host::{Dispatch, Host, Root, View};
use super::shader::Shader;
use super::{HostShader, validator};
use crate::native::HwStage;

#[test]
#[ignore = "requires Windows SDK dxil.dll and a D3D12 WARP device"]
fn two_loop_back_edges_preserve_both_phi_values_on_warp() {
    let mut s = Shader::new(false);
    s.threads = Some([64, 1, 1]);
    let entry = s.entry;
    let [head, choose, add, multiply, exit] = [(); 5].map(|()| s.m.new_block());
    let [zero, one, two, three, four, fifteen] = [0, 1, 2, 3, 4, 15].map(|n| s.m.i32(n));
    let tid = s.op(entry, "threadId.i32", Attr::ReadNone, s.t.i32, 93, &[zero]);
    let limit = s.m.bin(entry, Bin::And, tid, fifteen);
    s.m.terminate(entry, Term::Br(head));

    let i = s.m.phi(head, s.t.i32);
    let acc = s.m.phi(head, s.t.i32);
    let more = s.m.cmp(head, pred::IULT, i, limit);
    s.m.terminate(head, Term::CondBr(more, choose, exit));
    let bits = s.m.bin(choose, Bin::Xor, tid, i);
    let bit = s.m.bin(choose, Bin::And, bits, four);
    let first = s.m.cmp(choose, pred::EQ, bit, zero);
    s.m.terminate(choose, Term::CondBr(first, add, multiply));

    let i_plus_one = s.m.bin(add, Bin::Add, i, one);
    let sum = s.m.bin(add, Bin::Add, acc, tid);
    let sum = s.m.bin(add, Bin::Add, sum, one);
    s.m.terminate(add, Term::Br(head));
    let i_plus_two = s.m.bin(multiply, Bin::Add, i, two);
    let product = s.m.bin(multiply, Bin::Mul, acc, three);
    let product = s.m.bin(multiply, Bin::Add, product, i);
    s.m.terminate(multiply, Term::Br(head));
    s.m.set_phi(i, vec![(zero, entry), (i_plus_one, add), (i_plus_two, multiply)]);
    s.m.set_phi(acc, vec![(tid, entry), (sum, add), (product, multiply)]);

    // Store both outputs at separate offsets. A lane that skips the loop must still write its seed.
    let uav = s.heap_handle(exit, zero, false, false, (11 | 0x1000, 0));
    let byte_offset = s.m.bin(exit, Bin::Shl, tid, three);
    let second = s.m.bin(exit, Bin::Add, byte_offset, four);
    let undef = s.m.undef(s.t.i32);
    let mask = s.m.i8(1);
    for (at, value) in [(byte_offset, i), (second, acc)] {
        s.op(exit, "rawBufferStore.i32", Attr::NoUnwind, s.t.void, 140,
            &[uav, at, undef, value, undef, undef, undef, mask, four]);
    }
    s.m.terminate(exit, Term::Ret);
    let shader = HostShader {
        stage: HwStage::Cs, dxil: s.finish(), user_data: Vec::new(), samplers: Vec::new(),
        static_samplers: Vec::new(), root_cbvs: Vec::new(), root_srvs: Vec::new(),
        root_uavs: Vec::new(), root_indexed_views: Vec::new(),
    };
    validator::Validator::load(validator::Validator::default_path().as_deref())
        .expect("SDK validator required")
        .validate(&mut shader.dxil.clone()).expect("valid DXIL");
    let host = Host::new(true).expect("WARP required");
    let (root, pso) = host.compute_pipeline(&shader).expect("pipeline");
    let output = host.dispatch(&root, &pso, &Dispatch {
        roots: vec![(0, Root::Constants(vec![0]))],
        views: &[View::RawUav(256)], samplers: &[], groups: [2, 1, 1],
    }).expect("dispatch");
    let mut edges = [0, 0];
    for tid in 0..128u32 {
        let (mut i, mut acc) = (0, tid);
        while i < (tid & 15) {
            if (i ^ tid) & 4 == 0 {
                i += 1;
                acc = acc.wrapping_add(tid + 1);
                edges[0] += 1;
            } else {
                acc = acc.wrapping_mul(3).wrapping_add(i);
                i += 2;
                edges[1] += 1;
            }
        }
        let at = 2 * tid as usize;
        assert_eq!(&output[0][at..at + 2], &[i, acc], "thread {tid}");
    }
    assert!(edges.iter().all(|&n| n > 0));
    eprintln!("Both phi values matched on all 128 threads (256 dwords); CPU reference took the two edges {edges:?} times.");
}
