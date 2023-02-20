# nostr-relay-apigw-rust

![Overview](https://github.com/govm/nostr-relay-apigw-rust/blob/35d92a8ff010bf6912422c91980df42b8e678ed9/doc/overview.png)

[API Gateway](https://docs.aws.amazon.com/ja_jp/apigateway/latest/developerguide/apigateway-websocket-api.html) 
の背後の [Lambda](https://docs.aws.amazon.com/ja_jp/lambda/latest/dg/welcome.html) 上で動作する、
[DynamoDB](https://docs.aws.amazon.com/ja_jp/amazondynamodb/latest/developerguide/Introduction.html) に状態を保持する
[nostr](https://github.com/nostr-protocol/nips) relay です。

## 
This code is for PoC. You should never use it for production purposes.

## NIP

- [x] NIP-01: [Basic protocol flow description](https://github.com/nostr-protocol/nips/blob/master/01.md)
  - ただし、ids も authors も指定しない filter に対して　Stored Events を返しません
- [x] NIP-02: [Contact List and Petnames](https://github.com/nostr-protocol/nips/blob/master/02.md)
- [x] NIP-09: [Event Deletion](https://github.com/nostr-protocol/nips/blob/master/09.md)
- [x] NIP-11: [Relay Information Document](https://github.com/nostr-protocol/nips/blob/master/11.md)
- [x] NIP-15: [End of Stored Events Notice](https://github.com/nostr-protocol/nips/blob/master/15.md)
- [x] NIP-16: [Event Treatment](https://github.com/nostr-protocol/nips/blob/master/16.md)
- [x] NIP-20: [Command Results](https://github.com/nostr-protocol/nips/blob/master/20.md)

## Deploy
```sh
% cargo lambda build --release --arm64
% cargo lambda deploy
```

しかし次のものも必要です。
- WebSocket 用 API Gateway
- HTTP 用 API Gateway (for NIP-11)
- DynamoDB (event用, subscription用の2つのテーブル)
- CloudFront (wss と nip-11 用の接続を同じエンドポイントで受け付けるようにみせかけるために、CloudFront で受けて CloudFront Functions でうまくやるとよい)

## Hint

### Lambda には次の環境変数を与えるとよい
- NOSTR_EVENT_TABLE: Event用のテーブル名
- NOSTR_EVENT_TTL: Event用テーブルのレコードのTTL(秒)
- NOSTR_SUBSCRIPTION_TABLE: Subscription用のテーブル名
- NOSTR_SUBSCRIPTION_TTL: Subscription用テーブルのレコードのTTL(秒)

### DynmoDB には次のテーブルを作成するとよい
- Event用テーブル
  - Primary Key
    - Partition Key: id (String)
    - Sort Key: type (String)
  - GSI: pubkey-created_at-index
    -  Partition Key: pubkey (String)
    -  Sort Key: created_id (Number)
    -  projected attributes: id, kind
  - TTL: _ttl
- Event用テーブル
  - Primary Key
    - Partition Key: id (String)
    - Sort Key: type (String)
  - GSI: value-id-index
    -  Partition Key: value (String)
    -  Sort Key: id (String)
    -  projected attributes: Only Keys
  - TTL: _ttl

## API Gateway で次のようなAPIを作成するとよい
- WebSokcet 用 API
  - ルート式: $request.body.[0]
  - ルート: これらのルートの宛先を作成した Lambda 関数にする
    - REQ
    - EVENT
    - CLOSE
    - $disconnect
- HTTP 用 API
  - WebSocket 用 API が HTTP を受け取れないための措置
  - Lambda に向けとくと NIP-11 を応答します

## CloudFront を API Gateway の前段に置くと良い
次のような関数を設定するなどして、NIP-11のリクエストだけよろしくリダイレクトしてください
``` js
function handler(event) {
    var request = event.request;
    var headers = request.headers;
    var host = request.headers.host.value;
    
    if (headers.accept) {
        if (headers.accept.value == 'application/nostr+json') {
            var response =  {
                statusCode: 302,
                statusDescription: 'Found',
                headers: { 'location': { 'value': `https://${host}/nip11` }}
            }
            return response;
        }
    }
    return request;
}
```
