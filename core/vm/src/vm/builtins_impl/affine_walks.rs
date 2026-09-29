// Native kernel for the trapped-set survival experiment on the maps
// x -> x/2 (x even), x -> (3x + d)/2 (x odd).
//
// For every exactly-`bits`-bit start n whose parity walk has not contracted
// through step `bits` (3^q >= 2^t for every prefix), the kernel
//   * continues the real dynamics and counts survivors at each mark, and
//   * computes the exact fair-coin null: survival probabilities from each
//     state's odd count q by a backward recursion, giving mean and variance.
// Trapped states come from Terras's rule T^t(r + 2^t u) = T^t(r) + 3^q u, so
// only surviving prefixes are visited. Arithmetic is i128 and overflow-checked;
// states are processed in parallel and the counts are order-independent sums.

const AFFINE_LOG2_3: f64 = 1.584_962_500_721_156_2;

fn affine_floor(t: usize) -> Result<i64, String> {
    // smallest q with 3^q >= 2^t; equality is impossible for t >= 1
    if t == 0 {
        return Ok(0);
    }
    let x = t as f64 / AFFINE_LOG2_3;
    if (x - x.round()).abs() < 1e-9 {
        return Err(format!("affine_trapped_survival(): floor at t = {} is numerically ambiguous", t));
    }
    Ok(x.ceil() as i64)
}

fn affine_step(x: i128, d: i128) -> Option<i128> {
    if x % 2 == 0 {
        Some(x / 2)
    } else {
        x.checked_mul(3)?.checked_add(d).map(|v| v / 2)
    }
}

impl VM {
    fn bi_affine_trapped_survival(&mut self, args: Vec<Value>) -> Result<Value, String> {
        const NAME: &str = "affine_trapped_survival()";
        if args.len() != 3 {
            return Err(format!("{} expects 3 arguments, got {}", NAME, args.len()));
        }
        let bits = bytebuf_index_arg(&args[0], "affine_trapped_survival() bits")?;
        let d = args[1]
            .as_int()
            .ok_or_else(|| format!("{} offset must be int", NAME))?;
        if !(2..=40).contains(&bits) {
            return Err(format!("{} bits must be in 2..=40", NAME));
        }
        if d % 2 == 0 {
            return Err(format!("{} offset must be odd", NAME));
        }
        let marks_list = args[2]
            .as_list()
            .ok_or_else(|| format!("{} expects a list of marks", NAME))?;
        let mut marks: Vec<usize> = Vec::with_capacity(marks_list.len());
        for item in marks_list.iter() {
            let m = item.as_int().ok_or_else(|| format!("{} marks must be ints", NAME))?;
            if m <= bits as i64 || m > 4000 {
                return Err(format!("{} each mark must be in (bits, 4000]", NAME));
            }
            marks.push(m as usize);
        }
        let horizon = *marks.iter().max().ok_or_else(|| format!("{} needs at least one mark", NAME))?;
        let mut floors = Vec::with_capacity(horizon + 1);
        for t in 0..=horizon {
            floors.push(affine_floor(t)?);
        }
        let d128 = i128::from(d);

        // Depth-first enumeration of trapped residues via Terras's rule. The
        // lift keeps every tracked value positive when d is negative.
        let lift: i128 = 1i128 << (bits + 2);
        let low = 1i128 << (bits - 1);
        let mut states: Vec<(i128, i64)> = Vec::new(); // (start r, odd count q at step `bits`)
        let mut stack: Vec<(i128, usize, i128, i128, i64)> = vec![(0, 0, lift, 1, 0)];
        let mut visited: u64 = 0;
        while let Some((r, t, x, p3, q)) = stack.pop() {
            visited += 1;
            if t == bits {
                if r >= low {
                    states.push((r, q));
                }
                continue;
            }
            for b in 0..2i128 {
                let r2 = r + b * (1i128 << t);
                let x2 = x + b * p3;
                let odd = x2.rem_euclid(2) as i64;
                let q2 = q + odd;
                if q2 >= floors[t + 1] {
                    let next = affine_step(x2, d128).ok_or_else(|| format!("{} overflow in enumeration", NAME))?;
                    let p3n = if odd == 1 { p3.checked_mul(3).ok_or_else(|| format!("{} overflow", NAME))? } else { p3 };
                    stack.push((r2, t + 1, next, p3n, q2));
                }
            }
        }
        self.charge_work(visited)?;

        // Real dynamics from each state (parallel, order-independent counts).
        use rayon::prelude::*;
        let real: Result<Vec<Vec<u64>>, String> = states
            .par_iter()
            .map(|&(r, q0)| {
                let mut hits = vec![0u64; marks.len()];
                let mut x = r;
                for _ in 0..bits {
                    x = affine_step(x, d128).ok_or_else(|| "overflow".to_string())?;
                }
                let mut q = q0;
                let mut t = bits;
                while t < horizon {
                    q += x.rem_euclid(2) as i64;
                    x = affine_step(x, d128).ok_or_else(|| "overflow".to_string())?;
                    t += 1;
                    if q < floors[t] {
                        break;
                    }
                    for (i, &m) in marks.iter().enumerate() {
                        if m == t {
                            hits[i] += 1;
                        }
                    }
                }
                Ok(hits)
            })
            .collect();
        let real = real.map_err(|e| format!("{} {}", NAME, e))?;
        let mut observed = vec![0u64; marks.len()];
        for h in &real {
            for i in 0..marks.len() {
                observed[i] += h[i];
            }
        }

        // Exact fair-coin null by backward recursion for each mark.
        let mut out: Vec<Value> = Vec::new();
        out.push(Value::from_int(&mut self.gc, states.len() as i64));
        for (i, &mark) in marks.iter().enumerate() {
            let width = mark + 2;
            let mut row: Vec<f64> = (0..width).map(|q| if q as i64 >= floors[mark] { 1.0 } else { 0.0 }).collect();
            let mut t = mark;
            while t > bits {
                t -= 1;
                let mut prev = vec![0.0f64; width];
                for q in 0..width - 1 {
                    prev[q] = if (q as i64) < floors[t] { 0.0 } else { 0.5 * (row[q] + row[q + 1]) };
                }
                row = prev;
            }
            let mut mean = 0.0f64;
            let mut var = 0.0f64;
            for &(_, q) in &states {
                let p = row[(q as usize).min(width - 1)];
                mean += p;
                var += p * (1.0 - p);
            }
            out.push(Value::from_int(&mut self.gc, observed[i] as i64));
            out.push(Value::from_int(&mut self.gc, (mean * 1000.0).round() as i64));
            out.push(Value::from_int(&mut self.gc, (var * 1000.0).round() as i64));
        }
        self.charge_work(states.len() as u64 * horizon as u64)?;
        Ok(Value::list(&mut self.gc, out))
    }
}
