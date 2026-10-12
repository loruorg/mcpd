/**
 * The chrome's words.
 *
 * The rail, the colophon, the command well and the documentation furniture
 * appear on every page, so their text cannot live in the markup: nine locales
 * would mean nine copies of the same component. It lives here instead, keyed by
 * string id and then by locale, and the components read it.
 *
 * Two rules make an untranslated string impossible to ship quietly.
 *
 * 1. An entry must carry every locale. The `satisfies Catalogue` below is
 *    `Record<Locale, string>` per id, so leaving one out is a TypeScript error
 *    at `astro check` and at `tsc --noEmit`, not a fallback to English nobody
 *    notices.
 * 2. A locale that has the key but no translation carries `UNTRANSLATED`, and
 *    `t()` throws when it reads one. The site builds every page, so a locale
 *    page with a gap in the chrome fails the build and names the gap.
 *
 * Page prose is not here. Pages carry their own words; this is the furniture
 * around them. Nor are the languages themselves: a language is named in its own
 * language, which is the same string in all nine, so the names live in the
 * locale table with the codes.
 *
 * This file reads `@/lib/locales` rather than `@/lib/i18n`, which is the table
 * without the routing helpers. A gate that wants to count what is left to
 * translate can then import it under plain node, where the `astro:i18n` module
 * the helpers need does not exist.
 */

import { LOCALE_CODES, type Locale } from '@/lib/locales'

/**
 * What a locale carries until someone writes the translation.
 *
 * One shared constant rather than a copy of the English, so a gap is a gap by
 * identity and not by whether two languages happen to spell a word the same
 * way. `t()` refuses to render it. Nothing carries it today; it stays because
 * the next string added here will, between being written and being translated.
 */
export const UNTRANSLATED = '@@untranslated'

/** Every id carries every locale. Leaving one out fails to compile. */
type Catalogue = Record<string, Record<Locale, string>>

/**
 * The chrome, string by string.
 *
 * `{version}` and `{year}` are values the caller fills. Keep them, and move them
 * wherever the sentence needs them; a language that puts the year first is
 * translating, not breaking the template.
 *
 * To add a tenth language, add its code to `LOCALES` and then add that code to
 * every entry below. The compiler names each entry you missed, so the list of
 * what is left to write is the build error itself.
 *
 * ## What these translations decided, so the next one matches
 *
 * - **Not translated, in any locale.** `mcpd`, the six SDK names, `OTLP`,
 *   `OpenTelemetry`, `Docker`, `Kubernetes`, `HTTP`, `API`, and the `⌘C` glyph.
 *   They are identifiers a reader types or searches for.
 * - **`daemon`** is translated where the language has a settled term for the
 *   thing (`守护进程`, `デーモン`, `데몬`) and kept where the developer community
 *   writes the English word (fr, es, it, pt, tr). It is also the `d` in `mcpd`,
 *   so keeping it keeps the product name readable.
 * - **Portuguese is Brazilian.** `registra`, `você`, `Implantação`. Most
 *   Portuguese-language developer documentation is written this way, and the
 *   tag published for it is a plain `pt`, which serves both.
 * - **Chinese is Simplified, with mainland vocabulary**: `守护进程`, `菜单`,
 *   `可观测性`, `智能体`, `接入`.
 * - **Japanese is polite (ですます) in every full sentence**, and noun-final in
 *   labels, which is what a Japanese interface does. Korean holds the
 *   `-습니다` form for the same reason.
 * - **The rail says `Docs`, not the full word,** in fr, es, it and pt. All four
 *   languages use it in navigation, and the rail is the one piece of chrome
 *   with no room to spare: at 768px the English row already fills it. The
 *   unabbreviated word is `doc.nav`, which labels the documentation index.
 * - **Turkish prefers the shortest correct form**, because a suffix is not
 *   optional: `Belgeler`, `Arayüz`, `Kaynak`. Anything set in `.legend` is
 *   uppercased by CSS under `lang="tr"`, where `i` becomes `İ`, so every legend
 *   here is a Turkish word that is meant to case that way.
 */
export const UI_STRINGS = {
  /* ------------------------------------------------------------- the rail -- */
  'rail.home': {
    en: 'mcpd home',
    fr: 'Accueil mcpd',
    es: 'Inicio de mcpd',
    zh: 'mcpd 首页',
    ja: 'mcpd ホーム',
    ko: 'mcpd 홈',
    it: 'Home di mcpd',
    tr: 'mcpd ana sayfası',
    pt: 'Início do mcpd',
  },
  'rail.tagline': {
    en: 'durable runs and observability',
    fr: 'exécutions durables et observabilité',
    es: 'ejecuciones duraderas y observabilidad',
    zh: '持久化运行与可观测性',
    ja: '永続実行と可観測性',
    ko: '지속성 실행과 관측 가능성',
    it: 'esecuzioni durevoli e osservabilità',
    tr: 'kalıcı çalıştırma ve gözlemlenebilirlik',
    pt: 'execuções duráveis e observabilidade',
  },
  'rail.nav': {
    en: 'Main',
    fr: 'Principale',
    es: 'Principal',
    zh: '主导航',
    ja: 'メイン',
    ko: '메인',
    it: 'Principale',
    tr: 'Ana',
    pt: 'Principal',
  },
  'rail.docs': {
    en: 'Docs',
    fr: 'Docs',
    es: 'Docs',
    zh: '文档',
    ja: 'ドキュメント',
    ko: '문서',
    it: 'Docs',
    tr: 'Belgeler',
    pt: 'Docs',
  },
  'rail.quickstart': {
    en: 'Quickstart',
    fr: 'Prise en main',
    es: 'Inicio rápido',
    zh: '快速开始',
    ja: 'クイックスタート',
    ko: '빠른 시작',
    it: 'Avvio rapido',
    tr: 'Hızlı başlangıç',
    pt: 'Início rápido',
  },
  'rail.interface': {
    en: 'Interface',
    fr: 'Interface',
    es: 'Interfaz',
    zh: '界面',
    ja: 'インターフェース',
    ko: '인터페이스',
    it: 'Interfaccia',
    tr: 'Arayüz',
    pt: 'Interface',
  },
  'rail.deploy': {
    en: 'Deploy',
    fr: 'Déploiement',
    es: 'Despliegue',
    zh: '部署',
    ja: 'デプロイ',
    ko: '배포',
    it: 'Deploy',
    tr: 'Dağıtım',
    pt: 'Implantação',
  },
  'rail.source': {
    en: 'Source',
    fr: 'Source',
    es: 'Código',
    zh: '源码',
    ja: 'ソース',
    ko: '소스',
    it: 'Codice',
    tr: 'Kaynak',
    pt: 'Código',
  },
  'rail.menu': {
    en: 'Menu',
    fr: 'Menu',
    es: 'Menú',
    zh: '菜单',
    ja: 'メニュー',
    ko: '메뉴',
    it: 'Menu',
    tr: 'Menü',
    pt: 'Menu',
  },
  /* The tile is only ever shown lit, but the unlit label is what a reader with
     the interface open sees before the probe answers, so it is real text. */
  'rail.daemonAbsent': {
    en: 'no daemon',
    fr: 'aucun daemon',
    es: 'sin daemon',
    zh: '无守护进程',
    ja: 'デーモンなし',
    ko: '데몬 없음',
    it: 'nessun daemon',
    tr: 'daemon yok',
    pt: 'sem daemon',
  },
  'rail.daemonReady': {
    en: 'daemon {version}',
    fr: 'daemon {version}',
    es: 'daemon {version}',
    zh: '守护进程 {version}',
    ja: 'デーモン {version}',
    ko: '데몬 {version}',
    it: 'daemon {version}',
    tr: 'daemon {version}',
    pt: 'daemon {version}',
  },

  /* --------------------------------------------------- the language picker -- */
  /* The languages themselves are not here: a language is named in its own
     language, which is the same string in every locale. See `LOCALES`. */
  'lang.nav': {
    en: 'Language',
    fr: 'Langue',
    es: 'Idioma',
    zh: '语言',
    ja: '言語',
    ko: '언어',
    it: 'Lingua',
    tr: 'Dil',
    pt: 'Idioma',
  },

  /* --------------------------------------------------------- the colophon -- */
  'colophon.blurb': {
    en: "A daemon that holds your agent's runs between steps and records what each one did. Free, open source, and yours to run.",
    fr: 'Un daemon qui conserve les exécutions de votre agent entre les étapes et enregistre ce que chacune a fait. Gratuit, open source, et vous l’exécutez vous-même.',
    es: 'Un daemon que conserva las ejecuciones de tu agente entre pasos y registra lo que hizo cada una. Gratis, de código abierto, y lo ejecutas tú.',
    zh: '这个守护进程在步骤之间保存智能体的运行，并记录每次运行做了什么。免费、开源，由你自己运行。',
    ja: 'エージェントの実行をステップ間で保持し、各実行が何をしたかを記録するデーモンです。無料でオープンソース、自分で動かせます。',
    ko: '에이전트의 실행을 단계 사이에 보관하고 각 실행이 무엇을 했는지 기록하는 데몬입니다. 무료이고 오픈 소스이며, 직접 실행합니다.',
    it: 'Un daemon che conserva le esecuzioni del tuo agente tra un passo e l’altro e registra che cosa ha fatto ognuna. Gratuito, open source, e lo esegui tu.',
    tr: 'Ajanınızın çalıştırmalarını adımlar arasında saklayan, her birinin ne yaptığını kaydeden bir daemon. Ücretsiz, açık kaynak, kendiniz çalıştırırsınız.',
    pt: 'Um daemon que guarda as execuções do seu agente entre as etapas e registra o que cada uma fez. Gratuito, open source, e você é quem executa.',
  },
  /* The words of the holder are joined by U+00A0, not a plain space. The name is
     a registered company, so it is the same in all nine languages and it should
     not be broken across two lines; without the tie the footer column wrapped it
     as `Internet Technology / Services LLC`. Retyping this line by hand will put
     ordinary spaces back and the bad wrap with them. */
  'colophon.licence': {
    en: 'Apache-2.0 licence, {year} Internet Technology Services LLC',
    fr: 'Licence Apache-2.0, {year} Internet Technology Services LLC',
    es: 'Licencia Apache-2.0, {year} Internet Technology Services LLC',
    zh: 'Apache-2.0 许可证，{year} Internet Technology Services LLC',
    ja: 'Apache-2.0 ライセンス、{year} Internet Technology Services LLC',
    ko: 'Apache-2.0 라이선스, {year} Internet Technology Services LLC',
    it: 'Licenza Apache-2.0, {year} Internet Technology Services LLC',
    tr: 'Apache-2.0 lisansı, {year} Internet Technology Services LLC',
    pt: 'Licença Apache-2.0, {year} Internet Technology Services LLC',
  },
  'colophon.start': {
    en: 'Start',
    fr: 'Démarrer',
    es: 'Empezar',
    zh: '开始',
    ja: 'はじめに',
    ko: '시작',
    it: 'Iniziare',
    tr: 'Başlarken',
    pt: 'Começar',
  },
  'colophon.instrument': {
    en: 'Instrument',
    fr: 'Instrumenter',
    es: 'Instrumentar',
    zh: '接入',
    ja: '計装',
    ko: '계측',
    it: 'Strumentare',
    tr: 'Ölçümleme',
    pt: 'Instrumentar',
  },
  'colophon.reference': {
    en: 'Reference',
    fr: 'Référence',
    es: 'Referencia',
    zh: '参考',
    ja: 'リファレンス',
    ko: '레퍼런스',
    it: 'Riferimento',
    tr: 'Referans',
    pt: 'Referência',
  },
  'colophon.quickstart': {
    en: 'Quickstart',
    fr: 'Prise en main',
    es: 'Inicio rápido',
    zh: '快速开始',
    ja: 'クイックスタート',
    ko: '빠른 시작',
    it: 'Avvio rapido',
    tr: 'Hızlı başlangıç',
    pt: 'Início rápido',
  },
  'colophon.install': {
    en: 'Install',
    fr: 'Installation',
    es: 'Instalación',
    zh: '安装',
    ja: 'インストール',
    ko: '설치',
    it: 'Installazione',
    tr: 'Kurulum',
    pt: 'Instalação',
  },
  'colophon.workflows': {
    en: 'Workflows',
    fr: 'Workflows',
    es: 'Workflows',
    zh: '工作流',
    ja: 'ワークフロー',
    ko: '워크플로',
    it: 'Workflow',
    tr: 'İş akışları',
    pt: 'Workflows',
  },
  'colophon.deploy': {
    en: 'Docker and Kubernetes',
    fr: 'Docker et Kubernetes',
    es: 'Docker y Kubernetes',
    zh: 'Docker 与 Kubernetes',
    ja: 'Docker と Kubernetes',
    ko: 'Docker와 Kubernetes',
    it: 'Docker e Kubernetes',
    tr: 'Docker ve Kubernetes',
    pt: 'Docker e Kubernetes',
  },
  'colophon.interface': {
    en: 'The interface',
    fr: 'L’interface',
    es: 'La interfaz',
    zh: '界面',
    ja: 'インターフェース',
    ko: '인터페이스',
    it: 'L’interfaccia',
    tr: 'Arayüz',
    pt: 'A interface',
  },
  'colophon.dataModel': {
    en: 'Data model',
    fr: 'Modèle de données',
    es: 'Modelo de datos',
    zh: '数据模型',
    ja: 'データモデル',
    ko: '데이터 모델',
    it: 'Modello dei dati',
    tr: 'Veri modeli',
    pt: 'Modelo de dados',
  },
  'colophon.api': {
    en: 'HTTP API',
    fr: 'API HTTP',
    es: 'API HTTP',
    zh: 'HTTP API',
    ja: 'HTTP API',
    ko: 'HTTP API',
    it: 'API HTTP',
    tr: 'HTTP API',
    pt: 'API HTTP',
  },
  'colophon.otel': {
    en: 'OpenTelemetry',
    fr: 'OpenTelemetry',
    es: 'OpenTelemetry',
    zh: 'OpenTelemetry',
    ja: 'OpenTelemetry',
    ko: 'OpenTelemetry',
    it: 'OpenTelemetry',
    tr: 'OpenTelemetry',
    pt: 'OpenTelemetry',
  },
  'colophon.configuration': {
    en: 'Configuration',
    fr: 'Configuration',
    es: 'Configuración',
    zh: '配置',
    ja: '設定',
    ko: '설정',
    it: 'Configurazione',
    tr: 'Yapılandırma',
    pt: 'Configuração',
  },
  'colophon.source': {
    en: 'Source',
    fr: 'Source',
    es: 'Código',
    zh: '源码',
    ja: 'ソース',
    ko: '소스',
    it: 'Codice',
    tr: 'Kaynak',
    pt: 'Código',
  },
  'colophon.contributing': {
    en: 'Contributing',
    fr: 'Contribuer',
    es: 'Contribuir',
    zh: '参与贡献',
    ja: '貢献ガイド',
    ko: '기여하기',
    it: 'Contribuire',
    tr: 'Katkıda bulunma',
    pt: 'Contribuir',
  },
  'colophon.promise': {
    en: 'No telemetry. Run it yourself, or use it free at mcpd.sh.',
    fr: 'Pas de télémétrie. Hébergez-le vous-même, ou utilisez-le gratuitement sur mcpd.sh.',
    es: 'Sin telemetría. Alójalo tú mismo o úsalo gratis en mcpd.sh.',
    zh: '无遥测。可自行部署，也可在 mcpd.sh 免费使用。',
    ja: 'テレメトリーなし。自分で運用するか、mcpd.sh で無料で使えます。',
    ko: '텔레메트리 없음. 직접 운영하거나 mcpd.sh에서 무료로 사용하세요.',
    it: 'Nessuna telemetria. Gestiscilo tu, o usalo gratis su mcpd.sh.',
    tr: 'Telemetri yok. Kendiniz çalıştırın ya da mcpd.sh üzerinde ücretsiz kullanın.',
    pt: 'Sem telemetria. Hospede você mesmo ou use grátis em mcpd.sh.',
  },

  /* ------------------------------------------------------ the command well -- */
  'command.copy': {
    en: 'Copy',
    fr: 'Copier',
    es: 'Copiar',
    zh: '复制',
    ja: 'コピー',
    ko: '복사',
    it: 'Copia',
    tr: 'Kopyala',
    pt: 'Copiar',
  },
  'command.copied': {
    en: 'Copied',
    fr: 'Copié',
    es: 'Copiado',
    zh: '已复制',
    ja: 'コピー済み',
    ko: '복사됨',
    it: 'Copiato',
    tr: 'Kopyalandı',
    pt: 'Copiado',
  },
  /* Shown when the clipboard is refused, so it has to name the key the reader
     presses instead. That key is not the same on every keyboard, so the glyph
     stays put in every locale and only the sentence around it is translated. */
  'command.copyByHand': {
    en: 'Press ⌘C',
    fr: 'Appuyez sur ⌘C',
    es: 'Copia con ⌘C',
    zh: '按 ⌘C',
    ja: '⌘C でコピー',
    ko: '⌘C 누르기',
    it: 'Premi ⌘C',
    tr: '⌘C’ye basın',
    pt: 'Pressione ⌘C',
  },
  'command.copyLabel': {
    en: 'Copy the command',
    fr: 'Copier la commande',
    es: 'Copiar el comando',
    zh: '复制命令',
    ja: 'コマンドをコピー',
    ko: '명령 복사',
    it: 'Copia il comando',
    tr: 'Komutu kopyala',
    pt: 'Copiar o comando',
  },
  'command.methods': {
    en: 'Installation method',
    fr: 'Méthode d’installation',
    es: 'Método de instalación',
    zh: '安装方式',
    ja: 'インストール方法',
    ko: '설치 방법',
    it: 'Metodo di installazione',
    tr: 'Kurulum yöntemi',
    pt: 'Método de instalação',
  },
  /* The name given to a command whose program could not be read. */
  'command.unnamed': {
    en: 'Command',
    fr: 'Commande',
    es: 'Comando',
    zh: '命令',
    ja: 'コマンド',
    ko: '명령',
    it: 'Comando',
    tr: 'Komut',
    pt: 'Comando',
  },

  /* -------------------------------------------------- the documentation page -- */
  /* The furniture of `layouts/Doc.astro` and `layouts/Base.astro`. `doc.nav`
     labels the index of every page; `doc.previous` and `doc.next` are set in
     `.legend`, which uppercases through `text-transform`, and that is
     language-sensitive, so an English word left here would render on a Turkish
     page as `PREVİOUS`. */
  'doc.nav': {
    en: 'Documentation',
    fr: 'Documentation',
    es: 'Documentación',
    zh: '文档',
    ja: 'ドキュメント',
    ko: '문서',
    it: 'Documentazione',
    tr: 'Belgeler',
    pt: 'Documentação',
  },
  'doc.previous': {
    en: 'Previous',
    fr: 'Précédent',
    es: 'Anterior',
    zh: '上一页',
    ja: '前へ',
    ko: '이전',
    it: 'Precedente',
    tr: 'Önceki',
    pt: 'Anterior',
  },
  'doc.next': {
    en: 'Next',
    fr: 'Suivant',
    es: 'Siguiente',
    zh: '下一页',
    ja: '次へ',
    ko: '다음',
    it: 'Successivo',
    tr: 'Sonraki',
    pt: 'Próximo',
  },
  'doc.onThisPage': {
    en: 'On this page',
    fr: 'Sur cette page',
    es: 'En esta página',
    zh: '本页内容',
    ja: '目次',
    ko: '목차',
    it: 'In questa pagina',
    tr: 'Bu sayfada',
    pt: 'Nesta página',
  },
  /* ------------------------------------------------------------ figures -- */
  /* The isometric figures on the home page. Wire names inside a figure's
     readout (`span`, `started`, `orphaned`) stay as they are in every locale:
     they are what the reader will see in the API and the interface. */
  'fig.sorter.title': {
    en: 'Sorting bench',
    fr: 'Banc de tri',
    es: 'Banco de clasificación',
    zh: '分拣台',
    ja: '仕分け台',
    ko: '분류대',
    it: 'Banco di smistamento',
    tr: 'Ayırma tezgâhı',
    pt: 'Bancada de triagem',
  },
  'fig.sorter.caption': {
    en: 'Six kinds of signal arrive mixed on one belt. The reader reads the trace id on each, and every piece of a run lands in that run’s drawer.',
    fr: 'Six types de signaux arrivent mêlés sur un seul tapis. Le lecteur lit l’identifiant de trace de chacun, et chaque pièce d’une exécution tombe dans le tiroir de cette exécution.',
    es: 'Seis tipos de señal llegan mezclados en una sola cinta. El lector lee el id de traza de cada una, y cada pieza de una ejecución cae en el cajón de esa ejecución.',
    zh: '六种信号混在同一条传送带上到达。读取头读出每件上的 trace id，同一次运行的每一件都落进这次运行的抽屉。',
    ja: '6 種類のシグナルが 1 本のベルトに混ざって届きます。リーダーが各片の trace id を読み、同じ実行の片はすべてその実行の引き出しに入ります。',
    ko: '여섯 종류의 신호가 한 벨트에 섞여 도착합니다. 리더가 각 조각의 trace id를 읽고, 한 실행의 조각은 모두 그 실행의 서랍에 들어갑니다.',
    it: 'Sei tipi di segnale arrivano mescolati su un solo nastro. Il lettore legge il trace id di ciascuno, e ogni pezzo di un’esecuzione finisce nel cassetto di quell’esecuzione.',
    tr: 'Altı tür sinyal tek bir bantta karışık gelir. Okuyucu her birinin trace id’sini okur ve bir çalıştırmanın her parçası o çalıştırmanın çekmecesine düşer.',
    pt: 'Seis tipos de sinal chegam misturados em uma só esteira. O leitor lê o trace id de cada um, e cada peça de uma execução cai na gaveta dessa execução.',
  },
  'fig.sorter.hint': {
    en: 'Point at a drawer, or use the arrow keys',
    fr: 'Pointez un tiroir, ou utilisez les flèches',
    es: 'Señale un cajón, o use las flechas',
    zh: '指向一个抽屉，或使用方向键',
    ja: '引き出しを指すか、矢印キーを使います',
    ko: '서랍을 가리키거나 화살표 키를 쓰세요',
    it: 'Indica un cassetto, o usa le frecce',
    tr: 'Bir çekmeceyi gösterin ya da ok tuşlarını kullanın',
    pt: 'Aponte para uma gaveta, ou use as setas',
  },
  'fig.sorter.label': {
    en: 'A sorting bench. Spans, logs, metrics, scores, feedback and aspect notes ride one belt under a reader, which files each one into the drawer of the run that produced it. There are three drawers, one per run.',
    fr: 'Un banc de tri. Spans, logs, métriques, scores, retours et notes d’aspect passent sur un tapis sous un lecteur, qui range chacun dans le tiroir de l’exécution qui l’a produit. Il y a trois tiroirs, un par exécution.',
    es: 'Un banco de clasificación. Spans, logs, métricas, puntuaciones, opiniones y notas de aspecto pasan por una cinta bajo un lector, que archiva cada uno en el cajón de la ejecución que lo produjo. Hay tres cajones, uno por ejecución.',
    zh: '一个分拣台。span、日志、指标、评分、反馈和方面笔记在同一条传送带上经过读取头，读取头把每一件归入产生它的那次运行的抽屉。共有三个抽屉，每次运行一个。',
    ja: '仕分け台です。スパン、ログ、メトリクス、スコア、フィードバック、アスペクトノートが 1 本のベルトでリーダーの下を通り、それぞれを生成した実行の引き出しに入ります。引き出しは実行ごとに 1 つ、計 3 つです。',
    ko: '분류대입니다. 스팬, 로그, 메트릭, 점수, 피드백, 관점 노트가 한 벨트로 리더 아래를 지나고, 리더는 각각을 그것을 만든 실행의 서랍에 넣습니다. 서랍은 실행마다 하나씩 세 개입니다.',
    it: 'Un banco di smistamento. Span, log, metriche, punteggi, feedback e note di aspetto passano su un nastro sotto un lettore, che archivia ciascuno nel cassetto dell’esecuzione che l’ha prodotto. I cassetti sono tre, uno per esecuzione.',
    tr: 'Bir ayırma tezgâhı. Span, log, metrik, puan, geri bildirim ve görünüm notları tek bir bantta bir okuyucunun altından geçer; okuyucu her birini onu üreten çalıştırmanın çekmecesine koyar. Her çalıştırma için bir tane olmak üzere üç çekmece vardır.',
    pt: 'Uma bancada de triagem. Spans, logs, métricas, pontuações, feedback e notas de aspecto passam por uma esteira sob um leitor, que arquiva cada um na gaveta da execução que o produziu. Há três gavetas, uma por execução.',
  },
  'fig.lease.title': {
    en: 'Lease board',
    fr: 'Tableau des baux',
    es: 'Tablero de arriendos',
    zh: '租约板',
    ja: 'リース盤',
    ko: '리스 보드',
    it: 'Quadro dei lease',
    tr: 'Kira panosu',
    pt: 'Quadro de leases',
  },
  'fig.lease.caption': {
    en: 'The daemon keeps one tag per step on its board. A worker holds a tag only while its heartbeats arrive. Stop the worker and, after the lease and one reclaim window, the tag goes back on the board for the other worker.',
    fr: 'Le daemon garde une étiquette par étape sur son tableau. Un worker ne garde une étiquette que tant que ses battements arrivent. Arrêtez le worker : après le bail et une fenêtre de reprise, l’étiquette retourne au tableau pour l’autre worker.',
    es: 'El daemon guarda una etiqueta por paso en su tablero. Un worker conserva una etiqueta solo mientras llegan sus latidos. Detenga el worker y, tras el arriendo y una ventana de recuperación, la etiqueta vuelve al tablero para el otro worker.',
    zh: '守护进程在板上为每个步骤保留一个标签。worker 只有在心跳持续到达时才持有标签。停止该 worker，经过一个租约期和一个回收窗口后，标签回到板上，交给另一个 worker。',
    ja: 'デーモンはステップごとに 1 枚のタグを盤に掛けています。ワーカーがタグを持てるのはハートビートが届いている間だけです。ワーカーを止めると、リース期間と回収ウィンドウ 1 回分の後、タグは盤に戻り、もう一方のワーカーが取ります。',
    ko: '데몬은 단계마다 태그 하나를 보드에 걸어 둡니다. 워커는 하트비트가 도착하는 동안에만 태그를 쥡니다. 워커를 멈추면 리스 기간과 회수 창 한 번이 지난 뒤 태그가 보드로 돌아가 다른 워커가 가져갑니다.',
    it: 'Il daemon tiene sul quadro un’etichetta per passo. Un worker tiene un’etichetta solo finché arrivano i suoi heartbeat. Ferma il worker e, dopo il lease e una finestra di recupero, l’etichetta torna sul quadro per l’altro worker.',
    tr: 'Daemon panosunda her adım için bir etiket tutar. Bir worker etiketi yalnızca kalp atışları geldiği sürece elinde tutar. Worker’ı durdurun; kira süresi ve bir geri alma penceresinden sonra etiket diğer worker için panoya döner.',
    pt: 'O daemon mantém uma etiqueta por etapa no quadro. Um worker só segura uma etiqueta enquanto seus heartbeats chegam. Pare o worker e, depois do lease e de uma janela de recuperação, a etiqueta volta ao quadro para o outro worker.',
  },
  'fig.lease.hint': {
    en: 'Click, or press Space, to stop the worker',
    fr: 'Cliquez, ou appuyez sur Espace, pour arrêter le worker',
    es: 'Haga clic, o pulse Espacio, para detener el worker',
    zh: '点击或按空格键停止该 worker',
    ja: 'クリックするか Space キーでワーカーを止めます',
    ko: '클릭하거나 Space 키로 워커를 멈추세요',
    it: 'Fai clic, o premi Spazio, per fermare il worker',
    tr: 'Worker’ı durdurmak için tıklayın ya da Boşluk tuşuna basın',
    pt: 'Clique, ou pressione Espaço, para parar o worker',
  },
  'fig.lease.label': {
    en: 'A lease board. The daemon’s board holds a tag for each step of one run and a dial for the time since the last heartbeat. Two worker carriages on a rail take a tag, run the step and bring it back.',
    fr: 'Un tableau des baux. Le tableau du daemon porte une étiquette par étape d’une exécution et un cadran du temps écoulé depuis le dernier battement. Deux chariots worker sur un rail prennent une étiquette, exécutent l’étape et la rapportent.',
    es: 'Un tablero de arriendos. El tablero del daemon tiene una etiqueta por cada paso de una ejecución y un dial con el tiempo desde el último latido. Dos carros worker sobre un riel toman una etiqueta, ejecutan el paso y la devuelven.',
    zh: '一块租约板。守护进程的板上挂着一次运行中每个步骤的标签，还有一个表盘显示距上次心跳的时间。导轨上的两台 worker 小车取走标签，执行步骤，再把标签送回。',
    ja: 'リース盤です。デーモンの盤には 1 回の実行の各ステップのタグと、最後のハートビートからの経過時間を示すダイヤルがあります。レール上の 2 台のワーカー台車がタグを取り、ステップを実行して戻します。',
    ko: '리스 보드입니다. 데몬의 보드에는 한 실행의 단계마다 태그가 하나씩 있고, 마지막 하트비트 이후의 시간을 보여 주는 다이얼이 있습니다. 레일 위의 워커 캐리지 두 대가 태그를 가져가 단계를 실행하고 돌려놓습니다.',
    it: 'Un quadro dei lease. Il quadro del daemon porta un’etichetta per ogni passo di un’esecuzione e un quadrante con il tempo dall’ultimo heartbeat. Due carrelli worker su una rotaia prendono un’etichetta, eseguono il passo e la riportano.',
    tr: 'Bir kira panosu. Daemon’un panosunda bir çalıştırmanın her adımı için bir etiket ve son kalp atışından beri geçen süreyi gösteren bir kadran vardır. Raydaki iki worker arabası bir etiket alır, adımı çalıştırır ve geri getirir.',
    pt: 'Um quadro de leases. O quadro do daemon tem uma etiqueta para cada etapa de uma execução e um mostrador com o tempo desde o último heartbeat. Dois carros worker sobre um trilho pegam uma etiqueta, executam a etapa e a devolvem.',
  },
  'fig.drum.title': {
    en: 'Run drum',
    fr: 'Tambour d’exécution',
    es: 'Tambor de ejecución',
    zh: '运行记录鼓',
    ja: '実行ドラム',
    ko: '실행 드럼',
    it: 'Tamburo dell’esecuzione',
    tr: 'Çalıştırma tamburu',
    pt: 'Tambor da execução',
  },
  'fig.drum.caption': {
    en: 'The same run, wound round a recorder drum. Time runs round it, one track per span. Turn it, and the pen reads the span under it.',
    fr: 'La même exécution, enroulée sur un tambour enregistreur. Le temps en fait le tour, une piste par span. Tournez-le, et la plume lit le span sous elle.',
    es: 'La misma ejecución, enrollada en un tambor registrador. El tiempo le da la vuelta, una pista por span. Gírelo, y la plumilla lee el span que tiene debajo.',
    zh: '同一次运行，绕在记录鼓上。时间沿圆周展开，每个 span 一条轨道。转动它，笔尖读出下方的 span。',
    ja: '同じ実行を記録ドラムに巻いたものです。時間は周に沿って進み、スパンごとに 1 本のトラックがあります。回すと、ペンが真下のスパンを読みます。',
    ko: '같은 실행을 기록 드럼에 감은 것입니다. 시간은 둘레를 따라 흐르고, 스팬마다 트랙이 하나씩 있습니다. 돌리면 펜이 그 아래의 스팬을 읽습니다.',
    it: 'La stessa esecuzione, avvolta su un tamburo registratore. Il tempo gira intorno, una traccia per span. Giralo, e il pennino legge lo span sotto di sé.',
    tr: 'Aynı çalıştırma, bir kayıt tamburuna sarılmış. Zaman çevresi boyunca akar, her span için bir iz vardır. Çevirin; kalem altındaki span’i okur.',
    pt: 'A mesma execução, enrolada em um tambor registrador. O tempo dá a volta nele, uma trilha por span. Gire-o, e a pena lê o span embaixo dela.',
  },
  'fig.drum.hint': {
    en: 'Drag to turn the drum, or use the arrow keys',
    fr: 'Faites glisser pour tourner le tambour, ou utilisez les flèches',
    es: 'Arrastre para girar el tambor, o use las flechas',
    zh: '拖动以转动记录鼓，或使用方向键',
    ja: 'ドラッグしてドラムを回すか、矢印キーを使います',
    ko: '드래그해 드럼을 돌리거나 화살표 키를 쓰세요',
    it: 'Trascina per girare il tamburo, o usa le frecce',
    tr: 'Tamburu çevirmek için sürükleyin ya da ok tuşlarını kullanın',
    pt: 'Arraste para girar o tambor, ou use as setas',
  },
  'fig.drum.label': {
    en: 'A recorder drum carrying the seven spans of trace b3d9e71a, one track each, with a pen that reads the span at the current time.',
    fr: 'Un tambour enregistreur portant les sept spans de la trace b3d9e71a, une piste chacun, avec une plume qui lit le span à l’instant courant.',
    es: 'Un tambor registrador con los siete spans de la traza b3d9e71a, una pista cada uno, y una plumilla que lee el span del instante actual.',
    zh: '一个记录鼓，载有 trace b3d9e71a 的七个 span，每个一条轨道，笔尖读出当前时刻的 span。',
    ja: 'trace b3d9e71a の 7 つのスパンを 1 本ずつのトラックに載せた記録ドラムと、現在時刻のスパンを読むペンです。',
    ko: 'trace b3d9e71a의 스팬 일곱 개를 하나씩 트랙에 실은 기록 드럼과, 현재 시각의 스팬을 읽는 펜입니다.',
    it: 'Un tamburo registratore con i sette span della traccia b3d9e71a, una traccia ciascuno, e un pennino che legge lo span all’istante corrente.',
    tr: 'b3d9e71a izinin yedi span’ini her biri bir izde taşıyan bir kayıt tamburu ve o anki span’i okuyan bir kalem.',
    pt: 'Um tambor registrador com os sete spans do trace b3d9e71a, uma trilha cada, e uma pena que lê o span do instante atual.',
  },
  'base.skipToContent': {
    en: 'Skip to content',
    fr: 'Aller au contenu',
    es: 'Ir al contenido',
    zh: '跳到主要内容',
    ja: '本文へスキップ',
    ko: '본문으로 건너뛰기',
    it: 'Vai al contenuto',
    tr: 'İçeriğe geç',
    pt: 'Ir para o conteúdo',
  },
} satisfies Catalogue

export type StringId = keyof typeof UI_STRINGS

/* ------------------------------------------------------------ the reader -- */

/**
 * One chrome string, in one locale.
 *
 * `vars` fills the `{name}` slots. Pass nothing and the slots survive, which is
 * what the daemon tile wants: it hands the template to the browser and fills
 * `{version}` when the probe answers.
 *
 * It throws on an untranslated string rather than returning English. A page in
 * French with an English rail is a bug that nobody reports, because it looks
 * deliberate; a build that stops names the string and the locale.
 */
export function t(
  id: StringId,
  locale: Locale,
  vars?: Record<string, string | number>,
): string {
  const value = UI_STRINGS[id][locale]
  if (value === UNTRANSLATED) {
    throw new Error(
      `ui-strings: "${id}" has no ${locale} translation. ` +
        `Replace its ${locale} value in web/src/lib/ui-strings.ts, ` +
        `or do not build the ${locale} pages until the chrome is translated.`,
    )
  }
  if (!vars) return value
  return value.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  )
}

/** `t` with the locale already bound, for a component that reads many. */
export function strings(locale: Locale) {
  return (id: StringId, vars?: Record<string, string | number>) => t(id, locale, vars)
}

/**
 * Every string that is still waiting for a translation.
 *
 * A gate can call this and print the count per locale without building a page,
 * which is a cheaper answer than a build failure when the question is "how much
 * is left".
 */
export function missingTranslations(): { id: StringId; locale: Locale }[] {
  const gaps: { id: StringId; locale: Locale }[] = []
  for (const id of Object.keys(UI_STRINGS) as StringId[]) {
    for (const locale of LOCALE_CODES) {
      if (UI_STRINGS[id][locale] === UNTRANSLATED) gaps.push({ id, locale })
    }
  }
  return gaps
}
