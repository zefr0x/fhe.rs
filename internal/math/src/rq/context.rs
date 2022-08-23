use itertools::Itertools;
use num_bigint::BigUint;
use std::{fmt::Debug, sync::Arc};

use crate::{
	rns::RnsContext,
	zq::{ntt::NttOperator, Modulus},
	Error, Result,
};

/// Struct that holds the context associated with elements in rq.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Context {
	pub(crate) moduli: Vec<u64>,
	pub(crate) q: Vec<Modulus>,
	pub(crate) rns: Arc<RnsContext>,
	pub(crate) ops: Vec<NttOperator>,
	pub(crate) degree: usize,
	pub(crate) bitrev: Vec<usize>,
	pub(crate) inv_last_qi_mod_qj: Vec<u64>,
	pub(crate) inv_last_qi_mod_qj_shoup: Vec<u64>,
	pub(crate) next_context: Option<Arc<Context>>,
}

impl Debug for Context {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Context")
			.field("moduli", &self.moduli)
			// .field("q", &self.q)
			// .field("rns", &self.rns)
			// .field("ops", &self.ops)
			// .field("degree", &self.degree)
			// .field("bitrev", &self.bitrev)
			// .field("inv_last_qi_mod_qj", &self.inv_last_qi_mod_qj)
			// .field("inv_last_qi_mod_qj_shoup", &self.inv_last_qi_mod_qj_shoup)
			.field("next_context", &self.next_context)
			.finish()
	}
}

impl Context {
	/// Creates a context from a list of moduli and a polynomial degree.
	///
	/// Returns an error if the moduli are not primes less than 62 bits which
	/// supports the NTT of size `degree`.
	pub fn new(moduli: &[u64], degree: usize) -> Result<Self> {
		if !degree.is_power_of_two() || degree < 8 {
			Err(Error::Default(
				"The degree is not a power of two larger or equal to 8".to_string(),
			))
		} else {
			let mut q = Vec::with_capacity(moduli.len());
			let rns = Arc::new(RnsContext::new(moduli)?);
			let mut ops = Vec::with_capacity(moduli.len());
			for modulus in moduli {
				let qi = Modulus::new(*modulus)?;
				if let Some(op) = NttOperator::new(&qi, degree) {
					q.push(qi);
					ops.push(op);
				} else {
					return Err(Error::Default(
						"Impossible to construct a Ntt operator".to_string(),
					));
				}
			}
			let bitrev = (0..degree)
				.map(|j| (j.reverse_bits() >> (degree.leading_zeros() + 1)) as usize)
				.collect_vec();

			let mut inv_last_qi_mod_qj = vec![];
			let mut inv_last_qi_mod_qj_shoup = vec![];
			let q_last = moduli.last().unwrap();
			for qi in &q[..q.len() - 1] {
				let inv = qi.inv(qi.reduce(*q_last)).unwrap();
				inv_last_qi_mod_qj.push(inv);
				inv_last_qi_mod_qj_shoup.push(qi.shoup(inv));
			}

			let next_context = if moduli.len() >= 2 {
				Some(Arc::new(Context::new(&moduli[..moduli.len() - 1], degree)?))
			} else {
				None
			};

			Ok(Self {
				moduli: moduli.to_owned(),
				q,
				rns,
				ops,
				degree,
				bitrev,
				inv_last_qi_mod_qj,
				inv_last_qi_mod_qj_shoup,
				next_context,
			})
		}
	}

	/// Returns the modulus as a BigUint.
	pub fn modulus(&self) -> &BigUint {
		self.rns.modulus()
	}

	/// Returns a reference to the moduli in this context.
	pub fn moduli(&self) -> &[u64] {
		&self.moduli
	}

	/// Returns the number of iterations to switch to a children context.
	/// Returns an error if the context provided is not a child context.
	pub fn niterations_to(&self, context: &Arc<Context>) -> Result<usize> {
		if context.as_ref() == self {
			return Ok(0);
		}

		let mut niterations = 0;
		let mut found = false;
		let mut current_ctx = Arc::new(self.clone());
		while current_ctx.next_context.is_some() {
			niterations += 1;
			current_ctx = current_ctx.next_context.as_ref().unwrap().clone();
			if &current_ctx == context {
				found = true;
				break;
			}
		}
		if found {
			Ok(niterations)
		} else {
			Err(Error::InvalidContext)
		}
	}

	/// Returns the context after `i` iterations.
	pub fn context_at_level(&self, i: usize) -> Result<Arc<Self>> {
		if i >= self.moduli.len() {
			Err(Error::Default(
				"No context at the specified level".to_string(),
			))
		} else {
			let mut current_ctx = Arc::new(self.clone());
			for _ in 0..i {
				current_ctx = current_ctx.next_context.as_ref().unwrap().clone();
			}
			Ok(current_ctx)
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{error::Error, sync::Arc};

	use crate::{rq::Context, zq::ntt::supports_ntt};

	const MODULI: &[u64; 5] = &[
		1153,
		4611686018326724609,
		4611686018309947393,
		4611686018232352769,
		4611686018171535361,
	];

	#[test]
	fn test_context_constructor() {
		for modulus in MODULI {
			// modulus is = 1 modulo 2 * 8
			assert!(Context::new(&[*modulus], 8).is_ok());

			if supports_ntt(*modulus, 128) {
				assert!(Context::new(&[*modulus], 128).is_ok());
			} else {
				assert!(Context::new(&[*modulus], 128).is_err());
			}
		}

		// All moduli in MODULI are = 1 modulo 2 * 8
		assert!(Context::new(MODULI, 8).is_ok());

		// This should fail since 1153 != 1 moduli 2 * 128
		assert!(Context::new(MODULI, 128).is_err());
	}

	#[test]
	fn test_next_context() -> Result<(), Box<dyn Error>> {
		// A context should have a children pointing to a context with one less modulus.
		let context = Arc::new(Context::new(MODULI, 8)?);
		assert_eq!(
			context.next_context,
			Some(Arc::new(Context::new(&MODULI[..MODULI.len() - 1], 8)?))
		);

		// We can go down the chain of the MODULI.len() - 1 context's.
		let mut number_of_children = 0;
		let mut current = context.clone();
		while current.next_context.is_some() {
			number_of_children += 1;
			current = current.next_context.as_ref().unwrap().clone();
		}
		assert_eq!(number_of_children, MODULI.len() - 1);

		Ok(())
	}

	#[test]
	fn test_niterations_to() -> Result<(), Box<dyn Error>> {
		// A context should have a children pointing to a context with one less modulus.
		let context = Arc::new(Context::new(MODULI, 8)?);

		assert_eq!(context.niterations_to(&context).ok(), Some(0));

		assert_eq!(
			context
				.niterations_to(&Arc::new(Context::new(&MODULI[1..], 8)?))
				.err(),
			Some(crate::Error::InvalidContext)
		);

		for i in 1..MODULI.len() {
			assert_eq!(
				context
					.niterations_to(&Arc::new(Context::new(&MODULI[..MODULI.len() - i], 8)?))
					.ok(),
				Some(i)
			);
		}

		Ok(())
	}
}