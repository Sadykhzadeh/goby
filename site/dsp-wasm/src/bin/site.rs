//! Writes the website's pages, docs/index.html and docs/ru/index.html, from the app's
//! sources: its version and every voice's name and waveform, as the window draws them on
//! its tiles.
//!
//!   cargo run --release --bin site        (from site/dsp-wasm)
//!
//! One template, `PAGE`, is rendered once per language. Every text a visitor can read or
//! hear about, the scripts' too, is a row of `TEXTS`; the page holds `{{key}}` where the
//! row goes. The window on the page is goby.wasm, the library of this crate; until it
//! loads, and without JavaScript, docs/images/window.webp shows the same window.
#![allow(dead_code, unused_macros, unused_imports)]

#[path = "../../../../crates/dsp/src/dsp.rs"]
mod dsp;
#[path = "../../../../crates/dsp/src/formant.rs"]
mod formant;
#[path = "../../../../crates/dsp/src/fx.rs"]
mod fx;
#[path = "../../../../src/lang.rs"]
mod lang;
#[path = "../../../../src/voices.rs"]
mod voices;

/// The site's languages: the app's language code (the voices' names come from its file)
/// and where the page is written, relative to docs/.
const PAGES: [(&str, &str); 2] = [("en", "index.html"), ("ru", "ru/index.html")];

/// The size of the Windows release, in KB, as the README and both pages give it.
const SIZE_KB: &str = "374";

/// The hero eases between the voices' waveforms: [[name, [points]], ...], 60 points each, in
/// hundredths (the page divides by 100 again, so -0.44 is written -44).
fn hero() -> String {
    let rows: Vec<String> = voices::ALL
        .iter()
        .zip(voices::glyphs(60))
        .map(|(v, g)| {
            let points: Vec<String> = g.iter().map(|x| format!("{}", (x * 100.0).round() as i64)).collect();
            format!("[\"{}\",[{}]]", v.name(), points.join(","))
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// The scripts' texts, the rows of `TEXTS` whose key starts with `js.`, as a JSON object.
fn script_texts(at: usize) -> String {
    let rows: Vec<String> = TEXTS
        .iter()
        .filter_map(|row| Some((row.0.strip_prefix("js.")?, [row.1, row.2][at])))
        .map(|(key, text)| format!("\"{key}\":\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("{{{}}}", rows.join(","))
}

fn render(at: usize, version: &str) -> String {
    lang::choose(PAGES[at].0, "");
    let first = voices::ALL.iter().position(|v| v.id == "minion").expect("the Minion voice");
    let mut page = PAGE.to_string();
    for (key, en, ru) in TEXTS {
        let token = format!("{{{{{key}}}}}");
        assert!(key.starts_with("js.") || page.contains(&token), "{key} is in the table but not on the page");
        page = page.replace(&token, [en, ru][at]);
    }
    let total = voices::ALL.len();
    let noun = match at {
        0 => "voices",
        _ => russian_voices(total),
    };
    let page = page
        .replace("{{i18n}}", &script_texts(at))
        .replace("{first}", voices::ALL[first].name())
        .replace("{at}", &(first + 1).to_string())
        .replace("{voices.noun}", noun)
        .replace("{total}", &total.to_string())
        .replace("{kb}", SIZE_KB)
        .replace("{license}", env!("GOBY_LICENSE"))
        .replace("{version}", version)
        .replace("{hero}", &hero());
    assert!(!page.contains("{{"), "a key the table does not have");
    page
}

fn main() {
    let version = env!("GOBY_VERSION");
    let mut files: Vec<(String, String)> =
        PAGES.iter().enumerate().map(|(at, (_, path))| (path.to_string(), render(at, version))).collect();
    files.push(("sitemap.xml".to_string(), sitemap()));
    files.push(("robots.txt".to_string(), ROBOTS.to_string()));
    for (path, text) in files {
        let out = format!("{}/../../docs/{path}", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(std::path::Path::new(&out).parent().expect("a folder")).expect("docs folder");
        std::fs::write(&out, &text).unwrap_or_else(|e| panic!("docs/{path}: {e}"));
        println!("wrote docs/{path}, {} bytes", text.len());
    }
}

const ROBOTS: &str = "User-agent: *\nAllow: /\n\nSitemap: https://goby.azer.one/sitemap.xml\n";

/// The text row `key` in the language at `at`.
fn text_of(key: &str, at: usize) -> &'static str {
    let row = TEXTS.iter().find(|row| row.0 == key).expect("a text row");
    [row.1, row.2][at]
}

/// Both pages with their alternates, and the day git last changed each page's file.
fn sitemap() -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n",
    );
    for (at, (_, path)) in PAGES.iter().enumerate() {
        let lastmod = std::process::Command::new("git")
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .args(["log", "-1", "--format=%cs", "--", &format!("docs/{path}")])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|day| !day.is_empty());
        out += "  <url>\n";
        out += &format!("    <loc>{}</loc>\n", text_of("url", at));
        if let Some(day) = lastmod {
            out += &format!("    <lastmod>{day}</lastmod>\n");
        }
        for (alternate, (code, _)) in PAGES.iter().enumerate() {
            out += &format!(
                "    <xhtml:link rel=\"alternate\" hreflang=\"{code}\" href=\"{}\"/>\n",
                text_of("url", alternate)
            );
        }
        out += &format!("    <xhtml:link rel=\"alternate\" hreflang=\"x-default\" href=\"{}\"/>\n", text_of("url", 0));
        out += "  </url>\n";
    }
    out += "</urlset>\n";
    out
}

/// Every text of the page: its key, then the English and the Russian. A `js.` key is also
/// handed to the scripts. Russian keeps product names in Latin letters.
const TEXTS: &[(&str, &str, &str)] = &[
    // Where this page is, and where its twin is.
    ("lang", "en", "ru"),
    ("locale", "en_US", "ru_RU"),
    ("locale.other", "ru_RU", "en_US"),
    ("root", "", "../"),
    ("url", "https://goby.azer.one/", "https://goby.azer.one/ru/"),
    ("switch.href", "ru/", "../"),
    ("switch.lang", "ru", "en"),
    ("switch.label", "RU", "EN"),
    ("switch.title", "Русский", "English"),
    // The head.
    ("title", "Goby: a tiny voice changer for Windows", "Goby: маленькая программа для смены голоса в Windows"),
    (
        "description",
        "Real-time voice changer for Windows: {total} voices for the programs you pick, from Discord and OBS to calls and recordings. One {kb} KB app, free and open source, no account.",
        "Меняет голос в Windows в реальном времени: {total} {voices.noun} для программ, которые вы выберете, от Discord и OBS до звонков и записи. Одно приложение на {kb} КБ, бесплатное, с открытым кодом, без аккаунта.",
    ),
    (
        "og.description",
        "{total} voices for the programs you pick. One {kb} KB Windows app, free and open source.",
        "{total} {voices.noun} для программ, которые вы выберете. Одно приложение для Windows на {kb} КБ, бесплатное, с открытым кодом.",
    ),
    (
        "og.image_alt",
        "Goby: talk like anyone else. A free, open-source voice changer for Windows.",
        "Goby: говорите чужим голосом. Бесплатный голосовой меняла для Windows с открытым кодом.",
    ),
    // The navigation.
    ("nav.voices", "Voices", "Голоса"),
    ("nav.setup", "Setup", "Настройка"),
    ("nav.faq", "FAQ", "Вопросы"),
    // The hero.
    ("hero.title", "Talk like <span>anyone</span> else.", "Говорите <span>чужим</span> голосом."),
    ("hero.next.title", "Next voice", "Следующий голос"),
    ("hero.next", "next", "дальше"),
    (
        "hero.text",
        "<b>{total} voices</b> for the programs you pick, from Discord and OBS to calls and recordings, in one <b>{kb} KB</b> Windows app. Free, open source, no account.",
        "<b>{total} {voices.noun}</b> для программ, которые вы выберете: от Discord и OBS до звонков и записи. Всё в одном приложении для Windows на <b>{kb} КБ</b>. Бесплатно, с открытым кодом, без аккаунта.",
    ),
    ("hero.download", "Download Goby", "Скачать Goby"),
    ("hero.windows", "Windows 10 and 11", "Windows 10 и 11"),
    // The live window.
    (
        "live.title",
        "Try the real app right here. Nothing to install.",
        "Попробуйте приложение вживую. Скачивать не нужно.",
    ),
    (
        "live.nojs",
        "The live part needs JavaScript. The download above works without it.",
        "Живая часть работает только с JavaScript. Загрузка выше работает и без него.",
    ),
    ("js.try.off", "Try it with your microphone", "Попробовать с микрофоном"),
    ("js.try.on", "Stop listening", "Выключить микрофон"),
    ("js.sample.play", "Play a sample", "Послушать пример"),
    ("js.sample.stop", "Stop the sample", "Остановить пример"),
    ("js.sample.none", "Sample unavailable", "Пример недоступен"),
    (
        "js.sample.note",
        "Playing a sample. Pick voices and drag Pitch while it plays.",
        "Играет пример. Выбирайте голоса и двигайте «Тон», пока он звучит.",
    ),
    (
        "live.note",
        "Put headphones on first: through speakers your voice comes back into the mic. Nothing leaves your browser.",
        "Сначала наденьте наушники: через колонки ваш голос вернётся в микрофон. Звук остаётся в браузере.",
    ),
    (
        "live.shot",
        "Goby's main window: the Minion voice in the header above Pitch and Volume, and a gallery of the starred voices, each drawn as its own waveform.",
        "Главное окно Goby: голос «Миньон» над ползунками «Тон» и «Громкость» и галерея избранных голосов, у каждого своя форма волны.",
    ),
    (
        "js.window",
        "Goby's window. Arrow keys pick a voice; Escape goes back to the voices.",
        "Окно Goby. Стрелки выбирают голос, Escape возвращает к голосам.",
    ),
    (
        "js.unsupported",
        "This browser cannot run the voices live. A current Chrome, Edge, Firefox or Safari can.",
        "Этот браузер не умеет менять голос вживую. Подойдёт свежий Chrome, Edge, Firefox или Safari.",
    ),
    ("js.asking", "Your browser asks for the microphone…", "Браузер спрашивает разрешение на микрофон…"),
    (
        "js.listening",
        "Listening. Pick voices and drag Pitch while you talk.",
        "Слушаю. Выбирайте голоса и двигайте «Тон», пока говорите.",
    ),
    (
        "js.denied",
        "The microphone was not allowed, so there is nothing to hear yet.",
        "Микрофон не разрешён, так что пока слушать нечего.",
    ),
    ("js.nomic", "No microphone was found on this device.", "На этом устройстве нет микрофона."),
    ("js.failed", "The voices could not start here: ", "Голоса не удалось запустить: "),
    ("js.loadfail", "The voices could not load here: ", "Голоса не удалось загрузить: "),
    // Setup.
    ("setup.title", "Three minutes from download to talking.", "Три минуты от скачивания до разговора."),
    ("setup.1.title", "Get Goby", "Скачайте Goby"),
    (
        "setup.1.text",
        "One file. Choose <b>Install Goby</b>, or <b>Just run it</b> to try it without installing. No administrator rights, no account.",
        "Один файл. Выберите <b>Установить Goby</b> или <b>Просто запустить</b>, чтобы попробовать без установки. Права администратора и аккаунт не нужны.",
    ),
    ("setup.2.title", "Add the cable", "Добавьте кабель"),
    (
        "setup.2.text",
        "Goby reaches other programs through the free <a href=\"https://vb-audio.com/Cable/\">VB-Audio Virtual Cable</a>. Without it, Goby walks you through installing it and picks it up without a restart.",
        "Goby передаёт голос другим программам через бесплатный <a href=\"https://vb-audio.com/Cable/\">VB-Audio Virtual Cable</a>. Если его нет, Goby проведёт вас по установке и подхватит кабель без перезагрузки.",
    ),
    ("setup.3.title", "Pick the programs", "Выберите программы"),
    (
        "setup.3.text",
        "On <b>Programs</b>, press <b>+</b> next to Discord, OBS or any other program. Only they hear the voice; everything else keeps your real microphone. A program with its own microphone setting, like Telegram, needs it on <b>Default</b> once, and Goby tells you when.",
        "На странице <b>Программы</b> нажмите <b>+</b> рядом с Discord, OBS или любой другой программой. Голос слышат только они, всё остальное по-прежнему слышит ваш настоящий микрофон. Если у программы свой выбор микрофона, как у Telegram, один раз поставьте в нём <b>По умолчанию</b>: Goby подскажет, когда.",
    ),
    (
        "setup.shot",
        "Goby's Programs page: Counter-Strike 2 hearing Goby right now, and Telegram, Chrome, Steam, OBS and Discord offered with their own icons.",
        "Страница «Программы» в Goby: Counter-Strike 2 слышит Goby прямо сейчас, а Telegram, Chrome, Steam, OBS и Discord предложены со своими значками.",
    ),
    // The numbers.
    (
        "facts",
        "About <b>28 ms</b> from your mouth to the other program. Under <b>1 %</b> of one core, even for the heaviest voice.",
        "Около <b>28 мс</b> от вашего рта до другой программы. Меньше <b>1 %</b> одного ядра, даже на самый тяжёлый голос.",
    ),
    // Day to day.
    ("playing.title", "Stays out of the way.", "Не мешает."),
    (
        "playing.toggle",
        "effect on and off, even in a fullscreen app",
        "включить и выключить эффект, даже в полноэкранном приложении",
    ),
    ("playing.previous", "previous voice", "предыдущий голос"),
    ("playing.next", "next voice", "следующий голос"),
    (
        "playing.text",
        "Star the voices you use and the hotkeys cycle only those, while the tray menu lists them. Close the window and Goby keeps going in the tray; it starts with Windows only if you ask it to.",
        "Отметьте звёздочкой голоса, которыми пользуетесь, и горячие клавиши будут листать только их, а меню в трее покажет только их. Закройте окно, и Goby продолжит работать в трее; вместе с Windows он запускается, только если вы попросите.",
    ),
    (
        "playing.shot",
        "Goby's Settings page: microphone, noise gate, output, tray, startup, hotkeys and updates.",
        "Страница «Настройки» в Goby: микрофон, шумовой гейт, вывод, трей, автозапуск, горячие клавиши и обновления.",
    ),
    // Questions.
    ("faq.title", "Questions", "Вопросы"),
    ("faq.1.q", "Will an anti-cheat mind it?", "Не будет ли проблем с античитом?"),
    (
        "faq.1.a",
        "Goby never touches the game: it does not inject anything or read its memory. The game only sees one more microphone, made by VB-Audio's ordinary signed driver.",
        "Goby не трогает игру: ничего в неё не внедряет и не читает её память. Игра видит лишь ещё один микрофон, созданный обычным подписанным драйвером VB-Audio.",
    ),
    (
        "faq.2.q",
        "Can other programs still use my microphone?",
        "Смогут ли другие программы пользоваться моим микрофоном?",
    ),
    (
        "faq.2.a",
        "Yes, Goby shares it. <i>Low latency</i> under Settings &gt; Advanced takes it exclusively for about 11 ms less delay, and then nothing else can record from it while Goby runs.",
        "Да, Goby делит его с ними. <i>Низкая задержка</i> в Настройки &gt; Дополнительно занимает микрофон монопольно: задержка меньше примерно на 11 мс, но пока Goby работает, больше никто с него не запишет.",
    ),
    ("faq.3.q", "How do I know my copy is the real one?", "Как проверить, что у меня настоящая копия?"),
    (
        "faq.3.a",
        "Every release lists the SHA-256 of each file in <code>SHA256SUMS.txt</code>; compare it with <code>Get-FileHash</code> in PowerShell. Goby's own updater installs only releases signed with the author's key. Settings shows the version and the commit a copy says it was built from.",
        "К каждому релизу прилагается SHA-256 каждого файла в <code>SHA256SUMS.txt</code>; сверьте его через <code>Get-FileHash</code> в PowerShell. Собственное обновление Goby ставит только релизы, подписанные ключом автора. В настройках видно версию и коммит, из которого собрана ваша копия.",
    ),
    ("faq.4.q", "Why does it need VB-Cable?", "Зачем нужен VB-Cable?"),
    (
        "faq.4.a",
        "A program can only record from a microphone Windows knows about. VB-Cable is that microphone; Goby plays into it. Writing a driver of its own would mean a kernel component on your machine, and Goby would rather not.",
        "Программа может записывать только с микрофона, о котором знает Windows. VB-Cable и есть такой микрофон: Goby играет в него. Свой драйвер означал бы компонент ядра у вас в системе, а Goby этого не хочет.",
    ),
    ("faq.5.q", "Mac or Linux?", "Mac или Linux?"),
    (
        "faq.5.a",
        "No. Goby is built on Windows audio and the Windows UI, which is a large part of why it is small and fast.",
        "Нет. Goby построен на звуке и интерфейсе Windows, и во многом поэтому он такой маленький и быстрый.",
    ),
    // The footer.
    (
        "footer.credit",
        "done with <span aria-hidden=\"true\" class=\"heart\">♥</span> by azer.one",
        "done with <span aria-hidden=\"true\" class=\"heart\">♥</span> by azer.one",
    ),
    ("footer.releases", "All releases", "Все релизы"),
];

/// The noun after the voice count, in Russian as the count takes it: 1 голос, 2 голоса, 5 голосов.
fn russian_voices(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (1, m) if m != 11 => "голос",
        (2..=4, m) if !(12..=14).contains(&m) => "голоса",
        _ => "голосов",
    }
}

const PAGE: &str = r##"<!doctype html>
<html lang="{{lang}}">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://api.fontshare.com 'sha256-HWWQ6cOCnZFLKJ0LyI/+rcessfFL/BFkPuY2Ml/6AXc='; font-src https://cdn.fontshare.com; img-src 'self' data:; connect-src 'self'; media-src 'self'; base-uri 'none'; object-src 'none'; form-action 'none'">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{title}}</title>
<meta name="description" content="{{description}}">
<meta name="theme-color" content="#13100c">
<link rel="canonical" href="{{url}}">
<link rel="alternate" hreflang="en" href="https://goby.azer.one/">
<link rel="alternate" hreflang="ru" href="https://goby.azer.one/ru/">
<link rel="alternate" hreflang="x-default" href="https://goby.azer.one/">
<meta property="og:title" content="{{title}}">
<meta property="og:description" content="{{og.description}}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Goby">
<meta property="og:image" content="https://goby.azer.one/images/social.png">
<meta property="og:image:width" content="1280">
<meta property="og:image:height" content="640">
<meta property="og:image:alt" content="{{og.image_alt}}">
<meta property="og:url" content="{{url}}">
<meta property="og:locale" content="{{locale}}">
<meta property="og:locale:alternate" content="{{locale.other}}">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="{{root}}images/logo.svg" type="image/svg+xml">
<script type="application/ld+json">{"@context":"https://schema.org","@type":"SoftwareApplication","name":"Goby","description":"{{description}}","url":"{{url}}","inLanguage":"{{lang}}","operatingSystem":"Windows 10, Windows 11","applicationCategory":"MultimediaApplication","softwareVersion":"{version}","fileSize":"{kb} KB","license":"https://spdx.org/licenses/{license}.html","isAccessibleForFree":true,"offers":{"@type":"Offer","price":"0","priceCurrency":"USD"}}</script>
<link rel="preconnect" href="https://api.fontshare.com">
<link rel="preconnect" href="https://cdn.fontshare.com" crossorigin>
<link rel="stylesheet" href="https://api.fontshare.com/v2/css?f[]=pally@500,700&display=swap">
<link rel="stylesheet" href="{{root}}site.css">
<script src="{{root}}site.js" defer></script>
<noscript><style>#voices .try, #win { display: none; }</style></noscript>
</head>
<body>
<script type="application/json" id="hero-voices">{hero}</script>
<script type="application/json" id="texts">{{i18n}}</script>
<nav class="top">
  <a class="mark" href="#top"><img src="{{root}}images/logo.svg" alt="" width="26" height="26">goby</a>
  <a class="link" href="#voices">{{nav.voices}}</a>
  <a class="link" href="#setup">{{nav.setup}}</a>
  <a class="link" href="#faq">{{nav.faq}}</a>
  <a class="link" href="https://github.com/Sadykhzadeh/goby">GitHub</a>
  <a class="link" href="{{switch.href}}" hreflang="{{switch.lang}}" lang="{{switch.lang}}" title="{{switch.title}}">{{switch.label}}</a>
</nav>

<main class="wrap" id="top">
  <section class="hero">
    <h1>{{hero.title}}</h1>
    <div class="stage">
      <svg viewBox="0 0 1200 260" preserveAspectRatio="none" aria-hidden="true">
        <path class="echo" id="echo" d=""/>
        <path id="wave" d=""/>
      </svg>
      <button class="voice" id="voice" type="button" title="{{hero.next.title}}">
        <small id="count">{at} / {total}</small><span id="name">{first}</span><span class="next">{{hero.next}}</span>
      </button>
    </div>
    <div class="foot">
      <p>{{hero.text}}</p>
      <div class="get">
        <a class="download" href="https://github.com/Sadykhzadeh/goby/releases/latest">
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3v10M5.5 8.8 10 13.3l4.5-4.5M4 17h12"/></svg>
          {{hero.download}} {version}
        </a>
        <small>{{hero.windows}}</small>
      </div>
    </div>
  </section>

  <section id="voices" class="live">
    <div class="live-head">
      <h2>{{live.title}}</h2>
      <noscript><p>{{live.nojs}}</p></noscript>
      <div class="try">
        <div class="try-row">
          <button class="try-btn" id="try" type="button"><svg class="ic" viewBox="0 0 16 16" aria-hidden="true"><rect x="5.8" y="1.5" width="4.4" height="8" rx="2.2"/><path d="M3.3 7.6a4.7 4.7 0 0 0 9.4 0M8 12.3v2.2"/></svg><span id="try-label">{{js.try.off}}</span></button>
          <button class="sample" id="sample" type="button"><svg class="ic" viewBox="0 0 16 16" aria-hidden="true"><path class="play" d="M4.5 2.8v10.4L13 8z"/><rect class="stop" x="3.5" y="3.5" width="9" height="9" rx="1.6"/></svg><span id="sample-label">{{js.sample.play}}</span></button>
        </div>
        <p id="try-note" aria-live="polite">{{live.note}}</p>
      </div>
    </div>

  <div class="win" id="win">
    <div class="bar"><img src="{{root}}images/logo.svg" alt="" width="16" height="16"><span>Goby</span></div>
    <img class="shot" src="{{root}}images/window.webp" width="1000" height="680" loading="lazy" decoding="async" alt="{{live.shot}}">
    <div class="menu" id="menu" role="menu" hidden></div>
  </div>
  </section>

  <section id="setup" class="setup">
    <div class="steps">
      <h2>{{setup.title}}</h2>
      <div class="step">
        <h3>{{setup.1.title}}</h3>
        <p>{{setup.1.text}}</p>
      </div>
      <div class="step">
        <h3>{{setup.2.title}}</h3>
        <p>{{setup.2.text}}</p>
      </div>
      <div class="step">
        <h3>{{setup.3.title}}</h3>
        <p>{{setup.3.text}}</p>
      </div>
    </div>
    <figure class="side">
      <img src="{{root}}images/programs.webp" width="790" height="845" loading="lazy" decoding="async" alt="{{setup.shot}}">
    </figure>
  </section>

  <section class="facts">
    <p>{{facts}}</p>
  </section>

  <section class="playing">
    <div>
      <h2>{{playing.title}}</h2>
      <ul class="keys">
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>G</kbd><span>{{playing.toggle}}</span></li>
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>PgUp</kbd><span>{{playing.previous}}</span></li>
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>PgDn</kbd><span>{{playing.next}}</span></li>
      </ul>
      <p>{{playing.text}}</p>
    </div>
    <figure class="side">
      <img src="{{root}}images/settings.webp" width="790" height="950" loading="lazy" decoding="async" alt="{{playing.shot}}">
    </figure>
  </section>

  <section id="faq" class="faq">
    <h2>{{faq.title}}</h2>
    <details>
      <summary>{{faq.1.q}}</summary>
      <p>{{faq.1.a}}</p>
    </details>
    <details>
      <summary>{{faq.2.q}}</summary>
      <p>{{faq.2.a}}</p>
    </details>
    <details>
      <summary>{{faq.3.q}}</summary>
      <p>{{faq.3.a}}</p>
    </details>
    <details>
      <summary>{{faq.4.q}}</summary>
      <p>{{faq.4.a}}</p>
    </details>
    <details>
      <summary>{{faq.5.q}}</summary>
      <p>{{faq.5.a}}</p>
    </details>
  </section>
</main>

<footer>
  <div class="wrap links">
    <span>Goby {version} · GPL-3.0</span>
    <a href="https://github.com/Sadykhzadeh/goby">GitHub</a>
    <a href="https://github.com/Sadykhzadeh/goby/releases">{{footer.releases}}</a>
    <a class="credit" href="https://azer.one">{{footer.credit}}</a>
  </div>
  <span class="word" aria-hidden="true">goby</span>
</footer>
</body>
</html>
"##;
