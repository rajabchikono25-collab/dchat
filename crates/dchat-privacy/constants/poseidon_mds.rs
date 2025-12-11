// Poseidon MDS Matrix for dchat ZK Proofs
//
// MDS (Maximum Distance Separable) matrix for Poseidon over BN254
// Width: 3 (rate 2 + capacity 1)
//
// This is the standard circulant MDS matrix for width 3
// Ensuring maximum diffusion in the permutation
//
// DO NOT MODIFY - these constants are security-critical

// MDS matrix as Vec<Vec<Bn254Fr>> (3x3)
vec![
    vec![
        Bn254Fr::from(1u64),
        Bn254Fr::from(1u64),
        Bn254Fr::from(2u64),
    ],
    vec![
        Bn254Fr::from(1u64),
        Bn254Fr::from(2u64),
        Bn254Fr::from(1u64),
    ],
    vec![
        Bn254Fr::from(2u64),
        Bn254Fr::from(1u64),
        Bn254Fr::from(1u64),
    ],
]
