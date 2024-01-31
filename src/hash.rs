// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) DUSK NETWORK. All rights reserved.

use alloc::vec::Vec;

use dusk_bls12_381::BlsScalar;
use dusk_jubjub::JubJubScalar;
use dusk_safe::{Call, Sponge};

use crate::hades::ScalarPermutation;
use crate::Error;

#[cfg(feature = "zk")]
pub(crate) mod gadget;

/// The Domain Separation for Poseidon
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Domain {
    /// Domain to specify hashing of 4-arity merkle tree
    Merkle4,
    /// Domain to specify hashing of 2-arity merkle tree
    Merkle2,
    /// Domain to specify hash used for encryption
    Encryption,
    /// Domain to specify hash for any other input
    Other,
}

impl Domain {
    /// Encryption for the domain-separator are taken from section 4.2 of the
    /// paper adapted to u64.
    /// When `Other` is selected we set the domain-separator to zero. We can do
    /// this since the io-pattern will be encoded in the tag in any case,
    /// ensuring safety from collision attacks.
    pub const fn encoding(&self) -> u64 {
        match self {
            // 2^4 - 1
            Domain::Merkle4 => 0x0000_0000_0000_000f,
            // 2^2 - 1
            Domain::Merkle2 => 0x0000_0000_0000_0003,
            // 2^32
            Domain::Encryption => 0x0000_0001_0000_0000,
            // 0
            Domain::Other => 0x0000_0000_0000_0000,
        }
    }
}

fn io_pattern<T>(
    domain: Domain,
    input: &[&[T]],
    output_len: usize,
) -> Result<Vec<Call>, Error> {
    let mut io_pattern = Vec::new();
    // check total input length against domain
    let input_len = input.iter().fold(0, |acc, input| acc + input.len());
    match domain {
        Domain::Merkle2 if input_len != 2 || output_len != 1 => {
            return Err(Error::IOPatternViolation);
        }
        Domain::Merkle4 if input_len != 4 || output_len != 1 => {
            return Err(Error::IOPatternViolation);
        }
        _ => {}
    }
    for input in input.iter() {
        io_pattern.push(Call::Absorb(input.len()));
    }
    io_pattern.push(Call::Squeeze(output_len));

    Ok(io_pattern)
}

/// Hash any given input into one or several scalar using the Hades
/// permutation strategy. The Hash can absorb multiple chunks of input but will
/// only call `squeeze` once at the finalization of the hash.
/// The output length is set to 1 element per default, but this can be
/// overridden with [`Hash::output_len`].
pub struct Hash<'a> {
    domain: Domain,
    input: Vec<&'a [BlsScalar]>,
    output_len: usize,
}

impl<'a> Hash<'a> {
    /// Create a new hash.
    pub fn new(domain: Domain) -> Self {
        Self {
            domain,
            input: Vec::new(),
            output_len: 1,
        }
    }

    /// Override the length of the hash output (default value is 1).
    pub fn output_len(&mut self, output_len: usize) {
        self.output_len = output_len;
    }

    /// Update the hash input.
    pub fn update(&mut self, input: &'a [BlsScalar]) {
        self.input.push(input);
    }

    /// Finalize the hash.
    pub fn finalize(&self) -> Result<Vec<BlsScalar>, Error> {
        // generate the io-pattern
        let io_pattern = io_pattern(self.domain, &self.input, self.output_len)?;

        // set the domain-separator
        let domain_sep = self.domain.encoding();

        // Generate the hash using the sponge framework.
        // initialize the sponge
        let mut sponge =
            Sponge::start(ScalarPermutation::new(), io_pattern, domain_sep)?;
        // absorb the input
        for input in self.input.iter() {
            sponge.absorb(input.len(), input)?;
        }
        // squeeze the output
        sponge.squeeze(self.output_len)?;

        // return the result
        Ok(sponge.finish()?)
    }

    /// Finalize the hash and output the result as a `JubJubScalar` by
    /// truncating the `BlsScalar` output to 250 bits.
    pub fn finalize_truncated(&self) -> Result<Vec<JubJubScalar>, Error> {
        // bit-mask to 'cast' a bls-scalar result to a jubjub-scalar by
        // truncating the 6 highest bits
        const TRUNCATION_MASK: BlsScalar = BlsScalar::from_raw([
            0xffff_ffff_ffff_ffff,
            0xffff_ffff_ffff_ffff,
            0xffff_ffff_ffff_ffff,
            0x03ff_ffff_ffff_ffff,
        ]);

        // finalize the hash as bls-scalar
        let bls_output = self.finalize()?;

        Ok(bls_output
            .iter()
            .map(|bls| {
                JubJubScalar::from_raw((bls & &TRUNCATION_MASK).reduce().0)
            })
            .collect())
    }

    /// Digest an input and calculate the hash immediately
    pub fn digest(
        domain: Domain,
        input: &'a [BlsScalar],
    ) -> Result<Vec<BlsScalar>, Error> {
        let mut hash = Self::new(domain);
        hash.update(input);
        hash.finalize()
    }

    /// Digest an input and calculate the hash as jubjub-scalar immediately
    pub fn digest_truncated(
        domain: Domain,
        input: &'a [BlsScalar],
    ) -> Result<Vec<JubJubScalar>, Error> {
        let mut hash = Self::new(domain);
        hash.update(input);
        hash.finalize_truncated()
    }
}
