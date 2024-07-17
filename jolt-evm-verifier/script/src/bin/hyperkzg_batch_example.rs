use alloy_primitives::{hex, U256};
use alloy_sol_types::{sol, SolType};

use ark_bn254::Bn254;
use ark_ec::pairing::Pairing;
use ark_ff::BigInteger;
use ark_ff::PrimeField;
use ark_std::UniformRand;
use jolt_core::poly::commitment::commitment_scheme::{BatchType, CommitmentScheme};
use jolt_core::poly::commitment::hyperkzg::*;
use jolt_core::poly::dense_mlpoly::DensePolynomial;
use jolt_core::utils::transcript::ProofTranscript;
use rand_chacha;
use rand_core::SeedableRng;

fn main() {
    // Testing 2^12 ie 4096 elements
    // We replicate the behavior of the standard rust tests, but output
    // the proof and verification key to ensure it is verified in sol as well.

    let ell = 12;
    let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(ell as u64);

    let n = 1 << ell; // n = 2^ell

    let srs = HyperKZGSRS::setup(&mut rng, n);
    let (pk, vk): (HyperKZGProverKey<Bn254>, HyperKZGVerifierKey<Bn254>) = srs.trim(n);

    let point = (0..ell)
        .map(|_| <Bn254 as Pairing>::ScalarField::rand(&mut rng))
        .collect::<Vec<_>>();

    let mut polys = vec![];
    let mut evals = vec![];
    let mut commitments = vec![];
    let mut borrowed = vec![];
    for _ in 0..8 {
        let poly = DensePolynomial::new(
            (0..n)
                .map(|_| <Bn254 as Pairing>::ScalarField::rand(&mut rng))
                .collect::<Vec<_>>(),
        );
        let eval = poly.evaluate(&point);
        commitments.push(HyperKZG::commit(&pk, &poly).unwrap());
        polys.push(poly);
        evals.push(eval);
    }

    for poly in polys.iter() {
        borrowed.push(poly);
    }

    // prove an evaluation
    let mut prover_transcript = ProofTranscript::new(b"TestEval");
    let proof: HyperKZGProof<Bn254> = HyperKZG::batch_prove(
        &(pk, vk),
        borrowed.as_slice(),
        &point,
        &evals,
        BatchType::Big,
        &mut prover_transcript,
    );

    sol!(struct VK {
        uint256 VK_g1_x;
        uint256 VK_g1_y;
        uint256[] VK_g2;
        uint256[] VK_beta_g2;
    });
    sol!(struct HyperKZGProofSol {
        uint256[] com; // G1 points represented pairwise
        uint256[] w; // G1 points represented pairwise
        uint256[] v_ypos; // Three vectors of scalars which must be ell length
        uint256[] v_yneg;
        uint256[] v_y;
    });
    sol!(struct BatchedExample {
        VK vk;
        HyperKZGProofSol proof;
        uint256[] commitments;
        uint256[] point;
        uint256[] claims;
    });

    // We must invert the vk point on g2
    let g1 = vk.kzg_vk.g1;
    let g2 = -vk.kzg_vk.g2;
    let g2_sol = vec![
        U256::from_be_slice(&g2.x.c0.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2.x.c1.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2.y.c0.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2.y.c1.into_bigint().to_bytes_be()),
    ];
    let g2_beta = vk.kzg_vk.beta_g2;
    let g2_beta_sol = vec![
        U256::from_be_slice(&g2_beta.x.c0.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2_beta.x.c1.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2_beta.y.c0.into_bigint().to_bytes_be()),
        U256::from_be_slice(&g2_beta.y.c1.into_bigint().to_bytes_be()),
    ];

    let vk_sol = VK {
        VK_g1_x: U256::from_be_slice(&g1.x.into_bigint().to_bytes_be()),
        VK_g1_y: U256::from_be_slice(&g1.y.into_bigint().to_bytes_be()),
        VK_g2: g2_sol,
        VK_beta_g2: g2_beta_sol,
    };

    let mut com = vec![];
    let mut w = vec![];
    let ypos_scalar = proof.v[0].clone();
    let yneg_scalar = proof.v[1].clone();
    let y_scalar = proof.v[2].clone();

    // Horrible type conversion here, possibly theres an easier way
    let v_ypos = ypos_scalar
        .iter()
        .map(|i| U256::from_be_slice(i.into_bigint().to_bytes_be().as_slice()))
        .collect();
    let v_yneg = yneg_scalar
        .iter()
        .map(|i| U256::from_be_slice(i.into_bigint().to_bytes_be().as_slice()))
        .collect();
    let v_y = y_scalar
        .iter()
        .map(|i| U256::from_be_slice(i.into_bigint().to_bytes_be().as_slice()))
        .collect();

    for point in proof.com.iter() {
        com.push(U256::from_be_slice(&point.x.into_bigint().to_bytes_be()));
        com.push(U256::from_be_slice(&point.y.into_bigint().to_bytes_be()));
    }

    for point in proof.w.iter() {
        w.push(U256::from_be_slice(&point.x.into_bigint().to_bytes_be()));
        w.push(U256::from_be_slice(&point.y.into_bigint().to_bytes_be()));
    }

    let proof_sol = HyperKZGProofSol {
        com,
        w,
        v_ypos,
        v_yneg,
        v_y,
    };

    let mut encoded_commitments = vec![];
    for point in commitments.iter() {
        let x = U256::from_be_slice(&point.0.x.into_bigint().to_bytes_be());
        let y = U256::from_be_slice(&point.0.y.into_bigint().to_bytes_be());
        encoded_commitments.push(x);
        encoded_commitments.push(y);
    }

    let point_encoded = point
        .iter()
        .map(|i| U256::from_be_slice(i.into_bigint().to_bytes_be().as_slice()))
        .collect();
    let mut evals_encoded = vec![];
    for eval in evals.iter() {
        evals_encoded.push(U256::from_be_slice(&eval.into_bigint().to_bytes_be()));
    }

    let example = BatchedExample {
        proof: proof_sol,
        vk: vk_sol,
        commitments: encoded_commitments,
        point: point_encoded,
        claims: evals_encoded,
    };

    print!("{}", hex::encode(BatchedExample::abi_encode(&example)));
}
