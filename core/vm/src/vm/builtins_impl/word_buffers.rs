// Packed unsigned 32-bit word arrays stored in `bytebuf` values.
//
// These kernels give RAD programs native-speed bulk integer arithmetic over
// hundreds of millions of words while keeping RAD's value semantics: every
// kernel is pure, returns a fresh buffer, and never mutates its inputs.
// All arithmetic is exact. Scaling rounds toward zero (floor), additions
// and scalings trap on u32 overflow instead of wrapping, and every index is
// bounds-checked. Large inputs are split across rayon workers by fixed index
// ranges; each output word depends only on its own index, and reductions are
// associative, so results are identical for every worker count.

const WORD_BYTES: usize = 4;
const WORDS_PARALLEL_MIN: usize = 1 << 16;
const WORDS_CHUNK: usize = 1 << 14;

#[inline]
fn word_at(bytes: &[u8], index: usize) -> u32 {
    let at = index * WORD_BYTES;
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn words_arg<'a>(value: &'a Value, what: &str) -> Result<&'a [u8], String> {
    let bytes = value
        .as_bytebuf()
        .ok_or_else(|| format!("{} expects a bytebuf, got {}", what, value.type_name()))?;
    if bytes.len() % WORD_BYTES != 0 {
        return Err(format!(
            "{} expects a word buffer whose length is a multiple of 4 bytes, got {}",
            what,
            bytes.len()
        ));
    }
    Ok(bytes.as_slice())
}

fn words_u64_arg(value: &Value, what: &str) -> Result<u64, String> {
    let raw = value
        .as_int()
        .ok_or_else(|| format!("{} expects int, got {}", what, value.type_name()))?;
    u64::try_from(raw).map_err(|_| format!("{} must be non-negative", what))
}

fn words_value_arg(value: &Value, what: &str) -> Result<u32, String> {
    let raw = words_u64_arg(value, what)?;
    u32::try_from(raw).map_err(|_| format!("{} must fit in an unsigned 32-bit word", what))
}

fn words_arity(args: &[Value], expected: usize, name: &str) -> Result<(), String> {
    if args.len() != expected {
        return Err(format!("{}() expects {} arguments, got {}", name, expected, args.len()));
    }
    Ok(())
}

/// Fill `count` output words with `f(index)`. `f` returns `None` on overflow
/// or domain failure, which becomes the supplied error.
fn words_generate<F>(count: usize, error: &str, f: F) -> Result<Vec<u8>, String>
where
    F: Fn(usize) -> Option<u32> + Sync,
{
    let mut out = vec![0u8; count * WORD_BYTES];
    let fill = |(chunk_index, chunk): (usize, &mut [u8])| -> bool {
        let base = chunk_index * WORDS_CHUNK;
        for (offset, word) in chunk.chunks_exact_mut(WORD_BYTES).enumerate() {
            match f(base + offset) {
                Some(value) => word.copy_from_slice(&value.to_le_bytes()),
                None => return false,
            }
        }
        true
    };
    let ok = if count >= WORDS_PARALLEL_MIN {
        use rayon::prelude::*;
        out.par_chunks_mut(WORDS_CHUNK * WORD_BYTES)
            .enumerate()
            .map(fill)
            .reduce(|| true, |a, b| a && b)
    } else {
        out.chunks_mut(WORDS_CHUNK * WORD_BYTES).enumerate().all(fill)
    };
    if ok {
        Ok(out)
    } else {
        Err(error.to_string())
    }
}

/// Associative reduction over word indices `0..count`.
fn words_reduce<F, R>(count: usize, identity: u64, f: F, combine: R) -> u64
where
    F: Fn(usize) -> u64 + Sync,
    R: Fn(u64, u64) -> u64 + Sync + Send + Copy,
{
    if count >= WORDS_PARALLEL_MIN {
        use rayon::prelude::*;
        (0..count.div_ceil(WORDS_CHUNK))
            .into_par_iter()
            .map(|chunk| {
                let start = chunk * WORDS_CHUNK;
                let end = (start + WORDS_CHUNK).min(count);
                (start..end).fold(identity, |acc, i| combine(acc, f(i)))
            })
            .reduce(|| identity, combine)
    } else {
        (0..count).fold(identity, |acc, i| combine(acc, f(i)))
    }
}

impl VM {
    fn bi_words_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 2, "words_new")?;
        let count = bytebuf_index_arg(&args[0], "words_new() count")?;
        let value = words_value_arg(&args[1], "words_new() value")?;
        count
            .checked_mul(WORD_BYTES)
            .ok_or_else(|| "words_new() count is too large".to_string())?;
        self.charge_work(count as u64)?;
        let out = words_generate(count, "words_new() failed", |_| Some(value))?;
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    fn bi_words_len(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 1, "words_len")?;
        let words = words_arg(&args[0], "words_len()")?;
        Ok(Value::from_int(&mut self.gc, (words.len() / WORD_BYTES) as i64))
    }

    fn bi_words_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 2, "words_get")?;
        let words = words_arg(&args[0], "words_get()")?;
        let index = bytebuf_index_arg(&args[1], "words_get() index")?;
        let count = words.len() / WORD_BYTES;
        if index >= count {
            return Err(format!("words_get() index {} out of bounds (len {})", index, count));
        }
        let value = word_at(words, index);
        Ok(Value::from_int(&mut self.gc, i64::from(value)))
    }

    fn bi_words_set(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 3, "words_set")?;
        let count = words_arg(&args[0], "words_set()")?.len() / WORD_BYTES;
        let index = bytebuf_index_arg(&args[1], "words_set() index")?;
        let value = words_value_arg(&args[2], "words_set() value")?;
        if index >= count {
            return Err(format!("words_set() index {} out of bounds (len {})", index, count));
        }
        self.charge_work(count as u64)?;
        let mut out = args[0].as_bytebuf().expect("validated word buffer").clone();
        out[index * WORD_BYTES..(index + 1) * WORD_BYTES].copy_from_slice(&value.to_le_bytes());
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    /// out[i] = src[(a*i + b) mod modulus] for 0 <= i < count.
    fn bi_words_gather(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 5, "words_gather")?;
        let src = words_arg(&args[0], "words_gather()")?;
        let count = bytebuf_index_arg(&args[1], "words_gather() count")?;
        let a = words_u64_arg(&args[2], "words_gather() multiplier")?;
        let b = words_u64_arg(&args[3], "words_gather() offset")?;
        let modulus = words_u64_arg(&args[4], "words_gather() modulus")?;
        let len = (src.len() / WORD_BYTES) as u64;
        if modulus == 0 || modulus > len {
            return Err(format!(
                "words_gather() modulus {} must be in 1..={} (source length)",
                modulus, len
            ));
        }
        count
            .checked_mul(WORD_BYTES)
            .ok_or_else(|| "words_gather() count is too large".to_string())?;
        self.charge_work(count as u64)?;
        let step = a % modulus;
        let origin = b % modulus;
        let out = words_generate(count, "words_gather() failed", |i| {
            // Chunk-local incremental indexing would be faster, but the
            // closed form keeps every word independent of scheduling.
            let index = ((u128::from(step) * i as u128 + u128::from(origin)) % u128::from(modulus)) as usize;
            Some(word_at(src, index))
        })?;
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    fn words_binary(
        &mut self,
        args: Vec<Value>,
        name: &str,
        op: fn(u32, u32) -> Option<u32>,
    ) -> Result<Value, String> {
        words_arity(&args, 2, name)?;
        let left = words_arg(&args[0], name)?;
        let right = words_arg(&args[1], name)?;
        if left.len() != right.len() {
            return Err(format!(
                "{}() expects equal word lengths, got {} and {}",
                name,
                left.len() / WORD_BYTES,
                right.len() / WORD_BYTES
            ));
        }
        let count = left.len() / WORD_BYTES;
        self.charge_work(count as u64)?;
        let error = format!("{}() overflowed an unsigned 32-bit word", name);
        let out = words_generate(count, &error, |i| op(word_at(left, i), word_at(right, i)))?;
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    fn bi_words_add(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.words_binary(args, "words_add", |a, b| a.checked_add(b))
    }

    fn bi_words_min(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.words_binary(args, "words_min", |a, b| Some(a.min(b)))
    }

    /// out[i] = floor(src[i] * numerator / 2^shift), trapping on u32 overflow.
    fn bi_words_scale(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 3, "words_scale")?;
        let src = words_arg(&args[0], "words_scale()")?;
        let numerator = words_u64_arg(&args[1], "words_scale() numerator")?;
        let shift = words_u64_arg(&args[2], "words_scale() shift")?;
        if shift > 64 {
            return Err(format!("words_scale() shift {} must be at most 64", shift));
        }
        let count = src.len() / WORD_BYTES;
        self.charge_work(count as u64)?;
        let out = words_generate(count, "words_scale() overflowed an unsigned 32-bit word", |i| {
            let product = u128::from(word_at(src, i)) * u128::from(numerator);
            u32::try_from(product >> shift).ok()
        })?;
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    /// dst[start + stride*i] += src[i] for 0 <= i < len(src).
    fn bi_words_add_strided(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 4, "words_add_strided")?;
        let dst = words_arg(&args[0], "words_add_strided() destination")?;
        let src = words_arg(&args[1], "words_add_strided() source")?;
        let start = bytebuf_index_arg(&args[2], "words_add_strided() start")?;
        let stride = bytebuf_index_arg(&args[3], "words_add_strided() stride")?;
        let count = dst.len() / WORD_BYTES;
        let added = src.len() / WORD_BYTES;
        if stride == 0 {
            return Err("words_add_strided() stride must be positive".to_string());
        }
        if added > 0 {
            let last = (added - 1)
                .checked_mul(stride)
                .and_then(|x| x.checked_add(start))
                .ok_or_else(|| "words_add_strided() range is too large".to_string())?;
            if last >= count {
                return Err(format!(
                    "words_add_strided() writes index {} out of bounds (len {})",
                    last, count
                ));
            }
        }
        self.charge_work(count as u64)?;
        let out = words_generate(count, "words_add_strided() overflowed an unsigned 32-bit word", |i| {
            let base = word_at(dst, i);
            if i >= start && (i - start) % stride == 0 && (i - start) / stride < added {
                base.checked_add(word_at(src, (i - start) / stride))
            } else {
                Some(base)
            }
        })?;
        Ok(Value::bytebuf(&mut self.gc, out))
    }

    fn bi_words_min_value(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 1, "words_min_value")?;
        let words = words_arg(&args[0], "words_min_value()")?;
        let count = words.len() / WORD_BYTES;
        if count == 0 {
            return Err("words_min_value() of an empty word buffer".to_string());
        }
        self.charge_work(count as u64)?;
        let value = words_reduce(count, u64::MAX, |i| u64::from(word_at(words, i)), u64::min);
        Ok(Value::from_int(&mut self.gc, value as i64))
    }

    fn bi_words_max_value(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 1, "words_max_value")?;
        let words = words_arg(&args[0], "words_max_value()")?;
        let count = words.len() / WORD_BYTES;
        if count == 0 {
            return Err("words_max_value() of an empty word buffer".to_string());
        }
        self.charge_work(count as u64)?;
        let value = words_reduce(count, 0, |i| u64::from(word_at(words, i)), u64::max);
        Ok(Value::from_int(&mut self.gc, value as i64))
    }

    /// Number of indices with left[i] > right[i].
    fn bi_words_count_gt(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 2, "words_count_gt")?;
        let left = words_arg(&args[0], "words_count_gt()")?;
        let right = words_arg(&args[1], "words_count_gt()")?;
        if left.len() != right.len() {
            return Err("words_count_gt() expects equal word lengths".to_string());
        }
        let count = left.len() / WORD_BYTES;
        self.charge_work(count as u64)?;
        let value = words_reduce(
            count,
            0,
            |i| u64::from(word_at(left, i) > word_at(right, i)),
            |a, b| a + b,
        );
        Ok(Value::from_int(&mut self.gc, value as i64))
    }

    /// min (or max) over i of floor(num[i] * 2^shift / den[i]); every den[i] must be positive.
    fn words_ratio(&mut self, args: Vec<Value>, name: &str, want_max: bool) -> Result<Value, String> {
        words_arity(&args, 3, name)?;
        let num = words_arg(&args[0], name)?;
        let den = words_arg(&args[1], name)?;
        let shift = words_u64_arg(&args[2], &format!("{}() shift", name))?;
        if num.len() != den.len() || num.is_empty() {
            return Err(format!("{}() expects equal nonempty word lengths", name));
        }
        if shift > 30 {
            return Err(format!("{}() shift {} must be at most 30", name, shift));
        }
        let count = num.len() / WORD_BYTES;
        self.charge_work(count as u64)?;
        let zero = words_reduce(count, 0, |i| u64::from(word_at(den, i) == 0), |a, b| a + b);
        if zero > 0 {
            return Err(format!("{}() divides by a zero word", name));
        }
        let ratio = |i: usize| (u64::from(word_at(num, i)) << shift) / u64::from(word_at(den, i));
        let value = if want_max {
            words_reduce(count, 0, ratio, u64::max)
        } else {
            words_reduce(count, u64::MAX, ratio, u64::min)
        };
        Ok(Value::from_int(&mut self.gc, value as i64))
    }

    fn bi_words_min_ratio(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.words_ratio(args, "words_min_ratio", false)
    }

    fn bi_words_max_ratio(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.words_ratio(args, "words_max_ratio", true)
    }

    /// Order-sensitive 62-bit digest (FNV-1a over little-endian bytes).
    fn bi_words_digest(&mut self, args: Vec<Value>) -> Result<Value, String> {
        words_arity(&args, 1, "words_digest")?;
        let words = words_arg(&args[0], "words_digest()")?;
        self.charge_work((words.len() / WORD_BYTES) as u64)?;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in words {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(Value::from_int(&mut self.gc, (hash >> 2) as i64))
    }
}
