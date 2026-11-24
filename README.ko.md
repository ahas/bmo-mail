[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-mail

자체 API를 가진 하나의 서비스로서의 메일입니다. 로그인 계정마다 자기 메일함을 가지며, 메일함은 화면에서
추가하고, 계정 종류별 어댑터를 통해 읽고 쓰고 감시합니다. 첫 어댑터는 IMAP과 제출(submission)입니다. 목록의
스레드 묶기와 페이지 나누기를 직접 하고, 메일이 도착하면 필터 규칙을 실행하고, 보낸 사람의 로고를 찾고, 보낸
메시지를 계정의 실행 취소 기간 동안 붙잡아 두어 되돌릴 수 있게 하며, 무엇이 왔고 무엇이 바뀌었는지를 자체
이벤트 스트림으로 알립니다. 다른 서비스는 호출하지 않습니다. 호스팅 도구 `bmo/mail_sent`는 한 사람이 어느 날
보낸 메일을 그 사람의 PC에 있는 Outlook에서, 아니면 선택 사항인 페이지 토큰 서비스가 빌려주는 토큰으로
Microsoft Graph에서 읽으며, 에이전시, 수신기, 계정 서비스, 노드 서비스, 그리고 그 페이지 토큰 서비스를 거칩니다. 코딩 하네스는 MCP 서버 `bmo-mail mcp`와
`bmo-mail` 스킬로 이 서비스를 가져가고, 콘솔은 `docs/api.md`가 정한 대로 서비스가 매니페스트와 함께 제공하는
`client/`의 번들로 화면을 가져갑니다.

## 테스트

테스트베드의 `personal-data` 검사가 개인 정보 규칙(`PERSONAL-DATA.md`)을 지킵니다. 저장소에는 다음 테스트가
있습니다. 이전 보관함의 가져오기는 `tests/import.rs`에, 계정은 `tests/accounts.rs`에 있으며, 계정의 라우트, 한
소유자 키가 다른 소유자의 것에 닿지 않는지, 비밀의 봉인, 계정 파일의 가져오기를 다룹니다. 끊긴 연결 위의
감시자는 `tests/watcher.rs`에, POP3 계정은 `tests/pop3.rs`에 있으며, 테스트용 POP3 서버와 SMTP 서버를
상대로 받은편지함 목록, 첨부 파일이 있는 메일 읽기, `UIDL`로 새 메일 구분, 저장소에 임시보관한 답장 보내기를
다룹니다. 메일 서버는 `tests/servers.rs`에 있으며, 서버가 기록이 되기 전의 계정을 같은 설정마다 하나의 서버 기록으로
옮기기, 테스트용 계정 서비스의 위임 사슬로 소유자가 닿는 서버, 그 가운데서 고르는 계정, 테스트용 SMTP 서버를 상대로
한 계정 서비스의 시스템 메일 발송을 다룹니다. 콘솔 면은 `tests/console.rs`에 있으며, 벤더링한 bmo-ui 릴리스 자체의 읽기로 읽은
제공 매니페스트와, 콘솔의 자격 증명과 서비스의 토큰이 어디까지 닿는지를 다룹니다. 화면의 문구는
`tests/words.rs`에 있으며, 화면이 쓰는 모든 키가 자체 언어 팩의 것이거나 릴리스의 `wording-keys` 모듈이 나열한
것인지 확인합니다. 호스팅 도구는 `tests/sent_mail.rs`에 있으며, 도구 호스트가 호출하는 방식 그대로 에이전시,
노드 서비스, 페이지 토큰 서비스, Graph의 대역에 대해 호출하여, 누구의 사람과 어느 기계인지, 두 경로의 순서,
각 상위 서비스에 요청할 때 쓰는 토큰, 거부, 그리고 도구 폴더가 보관하는 스키마를 확인합니다. `cargo test`가 이
모두를 실행합니다. 콘솔 플러그인 번들은 브라우저 테스트를 `client/tests/`에 둡니다. 화면은 그곳의 작은 Nuxt
호스트 안에서 독립적으로 서며, 클라이언트의 개발 서버가 제공하고, 화면이 바꾼 내용을 유지하는 서비스 대역을
상대로 Chromium에서 마우스로 조작합니다. `tests/browser/mail-drag.test.ts`는 목록의 행을 메일 탐색의 폴더와
라벨로, 필터 규칙을 다른 위치로 옮기고, 페이지를 다시 불러와 각 결과를 읽습니다.
`tests/browser/mail-kind-actions.test.ts`는 메일을 옮기지 않는 종류의 계정에서 넓은 화면, 좁은 화면, 휴대폰
폭 모두 옮기는 동작이 보이지 않는지 확인합니다. `tests/browser/mail-servers.test.ts`는 넓은 화면과 휴대폰 폭에서
연결 테스트와 함께 자기 서버를 두고, 새 서버를 포함해 고른 서버로 계정을 추가합니다. 번들의 검사는 타입 검사 뒤에
이 테스트들을 실행합니다. 그 밖의 동작 테스트는 미뤄져 있습니다. 최소 서비스들이 새 저장소 위에 서면 이곳에서
작성하고 실행하며, 이전 동작이 무엇을 지켰는지는 `docs/inventory.md`가 나열합니다. 빌드 검사는
`cargo check --all-targets`와 번들의 타입 검사입니다.

## 구성

| 폴더 | 내용 |
|---|---|
| `src/` | 서비스, 클라이언트, OpenAPI 문서, MCP 서버와 그 명령. 자체 모듈로 나뉜 하나의 크레이트입니다 |
| `examples/` | `docs/openapi.json`이 보관하는 그대로 OpenAPI 문서를 출력하는 `openapi`, 그리고 호스팅 도구의 스키마를 도구 폴더에 쓰는 `tool_schemas` |
| `migrations/` | 저장소. 테이블은 이 구문들을 순서대로 실행해 만들어집니다 |
| `wording/` | 서비스 자체의 문구. 언어 하나당 파일 하나입니다 |
| `plugin/` | 하네스 플러그인의 스킬 `skills/bmo-mail/` |
| `tools/` | 호스팅 도구 `sent-mail/`: 매니페스트, 스키마, 설명서 |
| `client/` | 콘솔 플러그인 번들: 메일 화면. 독립된 프로젝트입니다 |
| `deploy/` | 서비스의 사용자 유닛 |
| `docs/` | 서비스가 하는 일에 대한 정본 문서와 OpenAPI 문서 |
| `tests/` | 현재 있는 동작 테스트 |

루트에는 Cargo 매니페스트, `VERSION`, `LICENSE`, `testbed.toml`, `install.toml`, `rustfmt.toml`, `CLAUDE.md`,
`PERSONAL-DATA.md`, `korean-ledger.json`, `settings.toml`, 그리고 배포가 주는 로컬 설정의 예시인
`settings.local.toml.example`이 있습니다.

## 콘솔 플러그인 번들

`client/`는 bmo-ui 위의 콘솔 번들로서 메일 화면을 담습니다. 이 번들은 bmo-chat의 콘솔 면(`docs/api.md`,
「The console plugin bundle」)을 통해 bmo-ui의 콘솔 플러그인 계약(bmo-ui의 `docs/console-bundles.md`)을 지킵니다.
`bun run bundle`은 bmo-ui의 번들 빌드로 `client/console/`에서 모듈, 스타일시트, 문구, 그리고 빌드 기반이 된
레이어를 빌드하고, 서비스는 선언이 만드는 매니페스트 옆에서 이것들을 제공합니다. bmo-chat이 고정한 bmo-ui
릴리스와 bmo-text의 TypeScript 패키지 팩이 자체 설치, 타입 검사, 빌드를 위해 `client/vendor/` 아래에 있으므로,
클론한 저장소에 다른 체크아웃이 필요 없습니다. 본문의 타입은 `client/shared/openapi.ts`가 정하며, 이 파일은
`bun run openapi`가 `docs/openapi.json`에서 생성합니다. 번들의 검사는 `client/` 안에서 실행하는 `bun run check`로,
고정된 설치, 타입 검사, 브라우저 테스트로 이루어집니다.

## 문서

| 문서 | 내용 |
|---|---|
| `docs/mail.md` | 메일이 하는 일: 계정, 감시자, 읽기, 쓰기, 필터, 아바타, 화면 |
| `docs/api.md` | 라우트, 이벤트 스트림, 클라이언트, 하네스 플러그인, 서비스 자체의 문구, 저장소, 명령, 설정, 콘솔 플러그인 번들 |
| `docs/openapi.json` | 서비스 자체의 타입에서 생성한 OpenAPI 3.1 계약 |
| `docs/inventory.md` | 이 저장소가 이전 저장소에서 가져온 것과 각 묶음의 개수 및 출처 |
| `PERSONAL-DATA.md` | 이 저장소에 절대 있어서는 안 되는 것: 사람의 이름, 주소, 경로, 기기나 네트워크 정보, 방 구성원이나 회사의 정보, 실제 대화의 한 줄, 그리고 이를 지키는 검사 |

## 버전

`VERSION`은 저장소의 버전이자 크레이트 자체의 버전을 담으며, 상태 확인, OpenAPI 문서, MCP 서버, 번들이 이
버전으로 답합니다. 버전은 bmo-chat의 `docs/conventions.md` 「Versions」가 정한 대로 소유자가 선언한
릴리스에서만 올라갑니다. 커밋, 배포, 계약 변경은 버전을 올리지 않으며, 배포는 커밋으로 구별합니다.
