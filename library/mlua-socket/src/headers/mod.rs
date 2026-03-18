use mlua::{Error, Lua, Table};

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Configure header field capitalization table
    let canonic = lua.create_table()?;
    canonic.raw_set("accept", "Accept")?;
    canonic.raw_set("accept-charset", "Accept-Charset")?;
    canonic.raw_set("accept-encoding", "Accept-Encoding")?;
    canonic.raw_set("accept-language", "Accept-Language")?;
    canonic.raw_set("accept-ranges", "Accept-Ranges")?;
    canonic.raw_set("action", "Action")?;
    canonic.raw_set("alternate-recipient", "Alternate-Recipient")?;
    canonic.raw_set("age", "Age")?;
    canonic.raw_set("allow", "Allow")?;
    canonic.raw_set("arrival-date", "Arrival-Date")?;
    canonic.raw_set("authorization", "Authorization")?;
    canonic.raw_set("bcc", "Bcc")?;
    canonic.raw_set("cache-control", "Cache-Control")?;
    canonic.raw_set("cc", "Cc")?;
    canonic.raw_set("comments", "Comments")?;
    canonic.raw_set("connection", "Connection")?;
    canonic.raw_set("content-description", "Content-Description")?;
    canonic.raw_set("content-disposition", "Content-Disposition")?;
    canonic.raw_set("content-encoding", "Content-Encoding")?;
    canonic.raw_set("content-id", "Content-ID")?;
    canonic.raw_set("content-language", "Content-Language")?;
    canonic.raw_set("content-length", "Content-Length")?;
    canonic.raw_set("content-location", "Content-Location")?;
    canonic.raw_set("content-md5", "Content-MD5")?;
    canonic.raw_set("content-range", "Content-Range")?;
    canonic.raw_set("content-transfer-encoding", "Content-Transfer-Encoding")?;
    canonic.raw_set("content-type", "Content-Type")?;
    canonic.raw_set("cookie", "Cookie")?;
    canonic.raw_set("date", "Date")?;
    canonic.raw_set("diagnostic-code", "Diagnostic-Code")?;
    canonic.raw_set("dsn-gateway", "DSN-Gateway")?;
    canonic.raw_set("etag", "ETag")?;
    canonic.raw_set("expect", "Expect")?;
    canonic.raw_set("expires", "Expires")?;
    canonic.raw_set("final-log-id", "Final-Log-ID")?;
    canonic.raw_set("final-recipient", "Final-Recipient")?;
    canonic.raw_set("from", "From")?;
    canonic.raw_set("host", "Host")?;
    canonic.raw_set("if-match", "If-Match")?;
    canonic.raw_set("if-modified-since", "If-Modified-Since")?;
    canonic.raw_set("if-none-match", "If-None-Match")?;
    canonic.raw_set("if-range", "If-Range")?;
    canonic.raw_set("if-unmodified-since", "If-Unmodified-Since")?;
    canonic.raw_set("in-reply-to", "In-Reply-To")?;
    canonic.raw_set("keywords", "Keywords")?;
    canonic.raw_set("last-attempt-date", "Last-Attempt-Date")?;
    canonic.raw_set("last-modified", "Last-Modified")?;
    canonic.raw_set("location", "Location")?;
    canonic.raw_set("max-forwards", "Max-Forwards")?;
    canonic.raw_set("message-id", "Message-ID")?;
    canonic.raw_set("mime-version", "MIME-Version")?;
    canonic.raw_set("original-envelope-id", "Original-Envelope-ID")?;
    canonic.raw_set("original-recipient", "Original-Recipient")?;
    canonic.raw_set("pragma", "Pragma")?;
    canonic.raw_set("proxy-authenticate", "Proxy-Authenticate")?;
    canonic.raw_set("proxy-authorization", "Proxy-Authorization")?;
    canonic.raw_set("range", "Range")?;
    canonic.raw_set("received", "Received")?;
    canonic.raw_set("received-from-mta", "Received-From-MTA")?;
    canonic.raw_set("references", "References")?;
    canonic.raw_set("referer", "Referer")?;
    canonic.raw_set("remote-mta", "Remote-MTA")?;
    canonic.raw_set("reply-to", "Reply-To")?;
    canonic.raw_set("reporting-mta", "Reporting-MTA")?;
    canonic.raw_set("resent-bcc", "Resent-Bcc")?;
    canonic.raw_set("resent-cc", "Resent-Cc")?;
    canonic.raw_set("resent-date", "Resent-Date")?;
    canonic.raw_set("resent-from", "Resent-From")?;
    canonic.raw_set("resent-message-id", "Resent-Message-ID")?;
    canonic.raw_set("resent-reply-to", "Resent-Reply-To")?;
    canonic.raw_set("resent-sender", "Resent-Sender")?;
    canonic.raw_set("resent-to", "Resent-To")?;
    canonic.raw_set("retry-after", "Retry-After")?;
    canonic.raw_set("return-path", "Return-Path")?;
    canonic.raw_set("sender", "Sender")?;
    canonic.raw_set("server", "Server")?;
    canonic.raw_set("smtp-remote-recipient", "SMTP-Remote-Recipient")?;
    canonic.raw_set("status", "Status")?;
    canonic.raw_set("subject", "Subject")?;
    canonic.raw_set("te", "TE")?;
    canonic.raw_set("to", "To")?;
    canonic.raw_set("trailer", "Trailer")?;
    canonic.raw_set("transfer-encoding", "Transfer-Encoding")?;
    canonic.raw_set("upgrade", "Upgrade")?;
    canonic.raw_set("user-agent", "User-Agent")?;
    canonic.raw_set("vary", "Vary")?;
    canonic.raw_set("via", "Via")?;
    canonic.raw_set("warning", "Warning")?;
    canonic.raw_set("will-retry-until", "Will-Retry-Until")?;
    canonic.raw_set("www-authenticate", "WWW-Authenticate")?;
    canonic.raw_set("x-mailer", "X-Mailer")?;

    // Configure module table
    let module = lua.create_table()?;
    module.set("canonic", canonic)?;

    // Preload module
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("socket.headers", module)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::error::Error;

    #[test]
    fn preload() -> Result<(), Box<dyn Error>> {
        let lua = Lua::new();
        super::preload(&lua)?;
        let accept_charset: String = lua
            .load("return require('socket.headers').canonic['accept-charset']")
            .eval()?;
        assert_eq!(accept_charset, "Accept-Charset");
        Ok(())
    }
}
