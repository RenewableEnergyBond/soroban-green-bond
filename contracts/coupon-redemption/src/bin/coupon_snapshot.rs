use coupon_redemption::{hash_leaf, hash_pair, CouponLeaf};
use serde_json::{json, Value};
use soroban_sdk::{Address, BytesN, Env};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read},
};

#[derive(Debug)]
struct Event {
    ledger: u32,
    transaction_index: u32,
    event_index: u32,
    kind: String,
    from: Option<String>,
    to: Option<String>,
    amount: i128,
}

fn argument(args: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    args.get(name)
        .cloned()
        .ok_or_else(|| format!("missing required argument --{name}"))
}

fn parse_event(value: &Value) -> Result<Event, String> {
    let string = |key: &str| -> Result<String, String> {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("event field {key} must be a string"))
    };
    let integer = |key: &str| -> Result<u32, String> {
        value
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|number| u32::try_from(number).ok())
            .ok_or_else(|| format!("event field {key} must be a u32"))
    };
    let amount = value
        .get("amount")
        .and_then(|amount| match amount {
            Value::String(number) => number.parse::<i128>().ok(),
            Value::Number(number) => number.to_string().parse::<i128>().ok(),
            _ => None,
        })
        .ok_or_else(|| "event field amount must be an integer".to_owned())?;

    Ok(Event {
        ledger: integer("ledger")?,
        transaction_index: integer("transaction_index")?,
        event_index: integer("event_index")?,
        kind: string("kind")?.to_ascii_lowercase(),
        from: value
            .get("from")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        to: value
            .get("to")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        amount,
    })
}

fn replay_snapshot(
    events_jsonl: &str,
    record_ledger: u32,
) -> Result<BTreeMap<String, i128>, String> {
    let mut events = events_jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line)
                .map_err(|error| format!("invalid event JSON: {error}"))?;
            parse_event(&value)
        })
        .collect::<Result<Vec<_>, _>>()?;
    events.sort_by_key(|event| (event.ledger, event.transaction_index, event.event_index));

    let mut balances = BTreeMap::<String, i128>::new();
    for event in events
        .into_iter()
        .filter(|event| event.ledger <= record_ledger)
    {
        if event.amount <= 0 {
            return Err("event amount must be positive".to_owned());
        }
        match event.kind.as_str() {
            "mint" => add_balance(&mut balances, event.to, event.amount)?,
            "transfer" => {
                let from = event
                    .from
                    .ok_or_else(|| "transfer event is missing from".to_owned())?;
                let to = event
                    .to
                    .ok_or_else(|| "transfer event is missing to".to_owned())?;
                subtract_balance(&mut balances, &from, event.amount)?;
                add_balance(&mut balances, Some(to), event.amount)?;
            }
            "burn" => {
                let from = event
                    .from
                    .ok_or_else(|| "burn event is missing from".to_owned())?;
                subtract_balance(&mut balances, &from, event.amount)?;
            }
            _ => return Err(format!("unsupported bond event kind: {}", event.kind)),
        }
    }

    balances.retain(|_, balance| *balance > 0);
    if balances.is_empty() {
        return Err("snapshot contains no positive holder balances".to_owned());
    }
    Ok(balances)
}

fn add_balance(
    balances: &mut BTreeMap<String, i128>,
    address: Option<String>,
    amount: i128,
) -> Result<(), String> {
    let address = address.ok_or_else(|| "mint event is missing to".to_owned())?;
    let balance = balances.entry(address).or_default();
    *balance = balance
        .checked_add(amount)
        .ok_or_else(|| "balance overflow while replaying events".to_owned())?;
    Ok(())
}

fn subtract_balance(
    balances: &mut BTreeMap<String, i128>,
    address: &str,
    amount: i128,
) -> Result<(), String> {
    let balance = balances.entry(address.to_owned()).or_default();
    *balance = balance
        .checked_sub(amount)
        .ok_or_else(|| "balance underflow while replaying events".to_owned())?;
    if *balance < 0 {
        return Err(format!("negative snapshot balance for {address}"));
    }
    Ok(())
}

fn amount_in_settlement_units(
    bond_balance: i128,
    period_seconds: u64,
    annual_rate_bps: u32,
    bond_decimals: u32,
    settlement_decimals: u32,
) -> Result<i128, String> {
    if bond_balance <= 0 || period_seconds == 0 || annual_rate_bps == 0 {
        return Ok(0);
    }
    if bond_decimals > 18 || settlement_decimals > 18 {
        return Err("decimal precision above 18 is unsupported".to_owned());
    }
    let bond_scale = 10_i128
        .checked_pow(bond_decimals)
        .ok_or_else(|| "bond decimal scale overflow".to_owned())?;
    let settlement_scale = 10_i128
        .checked_pow(settlement_decimals)
        .ok_or_else(|| "settlement decimal scale overflow".to_owned())?;
    let numerator = bond_balance
        .checked_mul(i128::from(annual_rate_bps))
        .and_then(|value| value.checked_mul(i128::from(period_seconds)))
        .and_then(|value| value.checked_mul(settlement_scale))
        .ok_or_else(|| "coupon numerator overflow".to_owned())?;
    let denominator = bond_scale
        .checked_mul(10_000)
        .and_then(|value| value.checked_mul(365 * 24 * 60 * 60))
        .ok_or_else(|| "coupon denominator overflow".to_owned())?;
    Ok(numerator / denominator)
}

fn hex(bytes: &BytesN<32>) -> String {
    bytes
        .to_array()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn create_artifact(
    env: &Env,
    balances: BTreeMap<String, i128>,
    expected_outstanding: i128,
    coupon_id: u32,
    record_ledger: u32,
    period_start: u64,
    period_end: u64,
    annual_rate_bps: u32,
    periods_per_year: u32,
    settlement_asset: String,
    bond_decimals: u32,
    settlement_decimals: u32,
) -> Result<Value, String> {
    let period_seconds = period_end
        .checked_sub(period_start)
        .filter(|duration| *duration > 0)
        .ok_or_else(|| "period end must be after period start".to_owned())?;
    if periods_per_year != 1 && periods_per_year != 2 && periods_per_year != 4 {
        return Err("periods_per_year must be 1, 2, or 4".to_owned());
    }

    let mut claims = Vec::new();
    let mut total_balance = 0_i128;
    let mut total_claimable = 0_i128;
    for (holder, balance) in balances {
        total_balance = total_balance
            .checked_add(balance)
            .ok_or_else(|| "snapshot total balance overflow".to_owned())?;
        let amount = amount_in_settlement_units(
            balance,
            period_seconds,
            annual_rate_bps,
            bond_decimals,
            settlement_decimals,
        )?;
        if amount > 0 {
            total_claimable = total_claimable
                .checked_add(amount)
                .ok_or_else(|| "total coupon amount overflow".to_owned())?;
            claims.push((holder, balance, amount));
        }
    }
    if claims.is_empty() {
        return Err("snapshot produces no positive rounded claims".to_owned());
    }
    if total_balance != expected_outstanding {
        return Err(format!(
            "snapshot outstanding supply {total_balance} does not match on-chain value {expected_outstanding}"
        ));
    }

    let aggregate_amount = amount_in_settlement_units(
        total_balance,
        period_seconds,
        annual_rate_bps,
        bond_decimals,
        settlement_decimals,
    )?;
    let rounding_remainder = aggregate_amount
        .checked_sub(total_claimable)
        .ok_or_else(|| "per-holder rounding exceeded aggregate amount".to_owned())?;

    let leaf_hashes = claims
        .iter()
        .map(|(holder, _, amount)| {
            let address = Address::from_str(env, holder);
            let leaf = CouponLeaf {
                coupon_id,
                holder: address,
                amount: *amount,
            };
            hash_leaf(env, &leaf)
        })
        .collect::<Vec<_>>();
    let root = merkle_root(env, &leaf_hashes);

    let output_claims = claims
        .iter()
        .enumerate()
        .map(|(index, (holder, balance, amount))| {
            let proof = merkle_proof(env, &leaf_hashes, index)
                .into_iter()
                .map(|node| hex(&node))
                .collect::<Vec<_>>();
            json!({
                "holder": holder,
                "snapshot_balance": balance,
                "amount": amount,
                "proof": proof,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "coupon_id": coupon_id,
        "record_ledger": record_ledger,
        "period_start_timestamp": period_start,
        "period_end_timestamp": period_end,
        "period_seconds": period_seconds,
        "day_count": "Actual/365 Fixed",
        "annual_rate_bps": annual_rate_bps,
        "periods_per_year": periods_per_year,
        "settlement_asset": settlement_asset,
        "bond_decimals": bond_decimals,
        "settlement_decimals": settlement_decimals,
        "total_snapshot_balance": total_balance,
        "expected_outstanding_supply": expected_outstanding,
        "total_claimable_amount": total_claimable,
        "rounding_remainder_retained_by_issuer": rounding_remainder,
        "merkle_root": hex(&root),
        "claims": output_claims,
    }))
}

fn merkle_root(env: &Env, leaves: &[BytesN<32>]) -> BytesN<32> {
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| {
                if pair.len() == 1 {
                    pair[0].clone()
                } else {
                    hash_pair(env, &pair[0], &pair[1])
                }
            })
            .collect();
    }
    level[0].clone()
}

fn merkle_proof(env: &Env, leaves: &[BytesN<32>], leaf_index: usize) -> Vec<BytesN<32>> {
    let mut proof = Vec::new();
    let mut level = leaves.to_vec();
    let mut index = leaf_index;
    while level.len() > 1 {
        let sibling = if index % 2 == 0 { index + 1 } else { index - 1 };
        if sibling < level.len() {
            proof.push(level[sibling].clone());
        }
        level = level
            .chunks(2)
            .map(|pair| {
                if pair.len() == 1 {
                    pair[0].clone()
                } else {
                    hash_pair(env, &pair[0], &pair[1])
                }
            })
            .collect();
        index /= 2;
    }
    proof
}

fn parse_args() -> Result<BTreeMap<String, String>, String> {
    let mut args = env::args().skip(1);
    let mut values = BTreeMap::new();
    while let Some(key) = args.next() {
        if !key.starts_with("--") {
            return Err(format!("unexpected argument: {key}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        values.insert(key.trim_start_matches("--").to_owned(), value);
    }
    Ok(values)
}

fn parse<T: std::str::FromStr>(args: &BTreeMap<String, String>, key: &str) -> Result<T, String> {
    argument(args, key)?
        .parse()
        .map_err(|_| format!("invalid value for --{key}"))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let input = argument(&args, "events")?;
    let record_ledger: u32 = parse(&args, "record-ledger")?;
    let expected_outstanding: i128 = parse(&args, "expected-outstanding")?;
    let coupon_id: u32 = parse(&args, "coupon-id")?;
    let period_start: u64 = parse(&args, "period-start")?;
    let period_end: u64 = parse(&args, "period-end")?;
    let annual_rate_bps: u32 = parse(&args, "annual-rate-bps")?;
    let periods_per_year: u32 = parse(&args, "periods-per-year")?;
    let bond_decimals: u32 = parse(&args, "bond-decimals")?;
    let settlement_decimals: u32 = args
        .get("settlement-decimals")
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| "invalid value for --settlement-decimals".to_owned())?
        .unwrap_or(7);
    let settlement_asset = argument(&args, "settlement-asset")?;

    let mut input_text = String::new();
    if input == "-" {
        io::stdin()
            .read_to_string(&mut input_text)
            .map_err(|error| error.to_string())?;
    } else {
        input_text = fs::read_to_string(input).map_err(|error| error.to_string())?;
    }
    let balances = replay_snapshot(&input_text, record_ledger)?;
    let env = Env::default();
    let artifact = create_artifact(
        &env,
        balances,
        expected_outstanding,
        coupon_id,
        record_ledger,
        period_start,
        period_end,
        annual_rate_bps,
        periods_per_year,
        settlement_asset,
        bond_decimals,
        settlement_decimals,
    )?;
    let output = serde_json::to_string_pretty(&artifact).map_err(|error| error.to_string())?;
    if let Some(path) = args.get("output") {
        fs::write(path, output).map_err(|error| error.to_string())?;
    } else {
        println!("{output}");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("coupon-snapshot: {error}");
        eprintln!("Use --events FILE.jsonl --record-ledger N --expected-outstanding BASE_UNITS --coupon-id N --period-start UNIX --period-end UNIX --annual-rate-bps N --periods-per-year 1|2|4 --bond-decimals N --settlement-asset C... [--settlement-decimals 7] [--output FILE]");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_snapshot_in_ledger_order_and_excludes_later_events() {
        let events = r#"
{"ledger":3,"transaction_index":0,"event_index":0,"kind":"transfer","from":"G-holder-a","to":"G-holder-b","amount":"40"}
{"ledger":1,"transaction_index":0,"event_index":0,"kind":"mint","to":"G-holder-a","amount":"100"}
{"ledger":4,"transaction_index":0,"event_index":0,"kind":"burn","from":"G-holder-b","amount":"10"}
"#;
        let snapshot = replay_snapshot(events, 3).unwrap();
        assert_eq!(snapshot.get("G-holder-a"), Some(&60));
        assert_eq!(snapshot.get("G-holder-b"), Some(&40));
    }

    #[test]
    fn calculates_actual_365_and_per_holder_rounding() {
        let quarter_seconds = 90 * 24 * 60 * 60;
        let one_holder = amount_in_settlement_units(1_000, quarter_seconds, 500, 0, 7).unwrap();
        assert_eq!(one_holder, 123_287_671);

        let aggregate = amount_in_settlement_units(2, quarter_seconds, 500, 0, 7).unwrap();
        let per_holder = 2 * amount_in_settlement_units(1, quarter_seconds, 500, 0, 7).unwrap();
        assert_eq!(aggregate - per_holder, 1);
    }

    #[test]
    fn generated_single_leaf_root_matches_contract_xdr_hash() {
        let env = Env::default();
        let holder = "GBX3MIAPKVCMK5BJB4FQTADGYMDW7OBOTZXAME3WR4VL4FMADWGTVZO2";
        let artifact = create_artifact(
            &env,
            BTreeMap::from([(holder.to_owned(), 1_000_i128)]),
            1_000,
            7,
            123,
            1_792_224_000,
            1_800_000_000,
            500,
            4,
            "CSETTLEMENT".to_owned(),
            0,
            7,
        )
        .unwrap();

        let expected = CouponLeaf {
            coupon_id: 7,
            holder: Address::from_str(&env, holder),
            amount: 123_287_671,
        };
        assert_eq!(
            artifact["merkle_root"].as_str().unwrap(),
            hex(&hash_leaf(&env, &expected))
        );
        assert_eq!(artifact["claims"][0]["proof"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn refuses_snapshot_that_does_not_match_on_chain_supply() {
        let env = Env::default();
        let balances = BTreeMap::from([(
            "GBX3MIAPKVCMK5BJB4FQTADGYMDW7OBOTZXAME3WR4VL4FMADWGTVZO2".to_owned(),
            1_000_i128,
        )]);
        let result = create_artifact(
            &env,
            balances,
            1_001,
            999,
            7,
            1_792_224_000,
            1_800_000_000,
            500,
            4,
            "CSETTLEMENT".to_owned(),
            0,
            7,
        );

        assert!(result
            .unwrap_err()
            .contains("does not match on-chain value"));
    }
}
