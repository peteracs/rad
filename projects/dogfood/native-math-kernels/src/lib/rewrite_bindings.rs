unsafe extern "C" fn rewrite_rank_search(args: *const u64, argc: usize) -> u64 {
    let result: Result<JsonValue, String> = (|| {
        let args = arg_slice(args, argc)?;
        exact_arity(args, 1, "rewrite_rank_search")?;
        let request = serde_json::from_str::<rewrite_rank::Request>(&string_arg(
            args,
            0,
            "rewrite_rank_search",
        )?)
        .map_err(|error| format!("rewrite_rank_search request: {error}"))?;
        serde_json::to_value(rewrite_rank::search(&request)?).map_err(|error| error.to_string())
    })();
    result.map_or_else(fail, return_json)
}
