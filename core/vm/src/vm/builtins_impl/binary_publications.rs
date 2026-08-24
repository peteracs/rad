impl VM {
    fn bi_publish_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let channel = args[0]
            .as_str()
            .ok_or_else(|| "publish_bytes() channel must be a string".to_string())?;
        let payload = args[1]
            .as_bytebuf()
            .ok_or_else(|| "publish_bytes() payload must be a bytebuf".to_string())?;
        if self.in_simulation_fork != 0 || self.is_worker {
            return Err(
                "publish_bytes() is forbidden in speculative or isolated execution".to_string(),
            );
        }
        self.binary_publications.publish(channel, payload)?;
        Ok(Value::NIL)
    }
}
