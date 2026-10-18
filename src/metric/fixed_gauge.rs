use super::{Metric, MetricBuilder};
use core::fmt;
use fixed::{
    types::extra::{LeEqU16, LeEqU32, LeEqU64, LeEqU8},
    FixedI16, FixedI32, FixedI64, FixedI8, FixedU16, FixedU32, FixedU64, FixedU8,
};
use portable_atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "serde")]
use serde::{Serialize, Serializer};

#[cfg(feature = "timestamp")]
use crate::timestamp::TimestampCell;

/// A gauge metric whose value is a fixed-point number from the [`fixed`]
/// crate.
///
/// This is similar to the [`Gauge`](super::Gauge) metric type, but its value
/// is a fixed-point number of type `F` rather than an `f64`. It is formatted
/// with `F`'s [`Display`](fmt::Display) implementation, which uses only
/// integer arithmetic. This is intended for platforms where the code for
/// formatting floating-point numbers takes too much space, or that lack
/// 64-bit hardware floating point.
#[derive(Debug)]
pub struct FixedGauge<F> {
    value: AtomicU64,
    recorded: AtomicBool,
    _value: core::marker::PhantomData<fn() -> F>,

    #[cfg(feature = "timestamp")]
    timestamp: Option<TimestampCell>,
}

/// A fixed-point number that a [`FixedGauge`] can hold: one of the [`fixed`]
/// crate's types with 64 bits or fewer.
pub trait FixedValue: Copy + fmt::Display {
    #[doc(hidden)]
    fn to_u64(self) -> u64;
    #[doc(hidden)]
    fn from_u64(bits: u64) -> Self;
}

macro_rules! impl_fixed_value {
    ($($Fixed:ident<$LeEq:ident>),+ $(,)?) => {
        $(
            impl<Frac: $LeEq> FixedValue for $Fixed<Frac> {
                fn to_u64(self) -> u64 {
                    // Sign-extend the signed types, so that `from_u64` can
                    // truncate the bits again.
                    self.to_bits() as u64
                }

                fn from_u64(bits: u64) -> Self {
                    Self::from_bits(bits as _)
                }
            }
        )+
    };
}

impl_fixed_value! {
    FixedI8<LeEqU8>,
    FixedI16<LeEqU16>,
    FixedI32<LeEqU32>,
    FixedI64<LeEqU64>,
    FixedU8<LeEqU8>,
    FixedU16<LeEqU16>,
    FixedU32<LeEqU32>,
    FixedU64<LeEqU64>,
}

// === impl FixedGauge ===

impl<F: FixedValue> FixedGauge<F> {
    const fn from_builder(builder: &MetricBuilder<'_>) -> Self {
        #[cfg(not(feature = "timestamp"))]
        let _ = builder;
        Self {
            value: AtomicU64::new(0),
            recorded: AtomicBool::new(false),
            _value: core::marker::PhantomData,
            #[cfg(feature = "timestamp")]
            timestamp: builder.mk_timestamp(),
        }
    }

    pub fn set_value(&self, value: F) {
        #[cfg(feature = "timestamp")]
        if let Some(ref timestamp) = self.timestamp {
            if !timestamp.update_if_ahead() {
                return;
            }
        }
        self.value.store(value.to_u64(), Ordering::Release);
        self.recorded.store(true, Ordering::Release);
    }

    pub fn value(&self) -> F {
        F::from_u64(self.value.load(Ordering::Acquire))
    }
}

impl<F: FixedValue> Metric for FixedGauge<F> {
    const TYPE: &'static str = "gauge";

    fn has_been_recorded(&self) -> bool {
        self.recorded.load(Ordering::Acquire)
    }

    fn fmt_metric<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        write!(writer, "{}", self.value())?;

        #[cfg(feature = "timestamp")]
        if let Some(now) = self.timestamp.as_ref().map(TimestampCell::timestamp) {
            write!(writer, " {now}",)?;
        }

        Ok(())
    }

    fn build(builder: &MetricBuilder<'_>) -> Self {
        Self::from_builder(builder)
    }
}

#[cfg(feature = "serde")]
impl<F: FixedValue + Serialize> Serialize for FixedGauge<F> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.has_been_recorded() {
            serializer.serialize_some(&self.value())
        } else {
            serializer.serialize_none()
        }
    }
}
