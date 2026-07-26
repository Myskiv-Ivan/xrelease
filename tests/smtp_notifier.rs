//! Integration test: SMTP notifier delivers to a local plain SMTP mock.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use xrelease::notify::smtp::{SmtpNotifier, SmtpNotifierOptions, SmtpTlsMode};
use xrelease::notify::{Event, Notifier};

fn sample_event() -> Event {
    Event {
        source_id: "github:org/app".into(),
        source_kind: "GitHub".into(),
        title: "app: v2.0.0".into(),
        body: "Release notes".into(),
        url: Some("https://example.test/r".into()),
        routing_tag: None,
    }
}

async fn read_line(stream: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    let mut buf = vec![0_u8; 4096];
    let n = stream.read(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

async fn run_smtp_mock(listener: TcpListener, captured: Arc<Mutex<String>>) {
    let (mut stream, _) = listener.accept().await.expect("accept");
    stream
        .write_all(b"220 mock SMTP ready\r\n")
        .await
        .expect("banner");

    let greeting = read_line(&mut stream).await.expect("ehlo read");
    assert!(greeting.to_ascii_uppercase().contains("EHLO"));
    stream
        .write_all(b"250-mock\r\n250 STARTTLS\r\n")
        .await
        .expect("ehlo reply");

    let mail_from = read_line(&mut stream).await.expect("mail from");
    assert!(mail_from.to_ascii_uppercase().contains("MAIL FROM"));
    stream.write_all(b"250 OK\r\n").await.expect("mail from ok");

    let rcpt = read_line(&mut stream).await.expect("rcpt to");
    assert!(rcpt.to_ascii_uppercase().contains("RCPT TO"));
    stream.write_all(b"250 OK\r\n").await.expect("rcpt ok");

    let data = read_line(&mut stream).await.expect("data");
    assert!(data.to_ascii_uppercase().contains("DATA"));
    stream
        .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
        .await
        .expect("data ok");

    let payload = read_line(&mut stream).await.expect("payload");
    *captured.lock().expect("lock") = payload.clone();
    stream
        .write_all(b"250 OK\r\n")
        .await
        .expect("message accepted");

    let quit = read_line(&mut stream).await.expect("quit");
    if quit.to_ascii_uppercase().contains("QUIT") {
        let _ = stream.write_all(b"221 Bye\r\n").await;
    }
}

#[tokio::test]
async fn smtp_notifier_should_deliver_subject_containing_title() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock smtp");
    let addr = listener.local_addr().expect("local addr");
    let captured = Arc::new(Mutex::new(String::new()));
    let captured_server = Arc::clone(&captured);

    let server = tokio::spawn(async move {
        run_smtp_mock(listener, captured_server).await;
    });

    let notifier = SmtpNotifier::new(&SmtpNotifierOptions {
        name: "smtp-test".into(),
        host: addr.ip().to_string(),
        port: addr.port(),
        username: None,
        password: String::new(),
        from: "releases@example.com".into(),
        to: vec!["team@example.com".into()],
        tls: SmtpTlsMode::Plain,
        subject_template: None,
        template: None,
        body_format: "text".into(),
    })
    .expect("notifier");

    notifier.notify(&sample_event()).await.expect("notify");

    server.await.expect("server join");
    let message = captured.lock().expect("lock").clone();
    assert!(
        message.contains("app: v2.0.0"),
        "expected subject/title in message body, got: {message}"
    );
}
