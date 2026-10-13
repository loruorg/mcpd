/**
 * The documentation navigation, in the eight translated languages.
 *
 * The structure lives in `docs.ts` and stays one list, so a page added there
 * appears in every language without anyone remembering to touch nine files.
 * Only the words are here, keyed by the route and by the English legend, so a
 * new entry shows its English title in a translated sidebar rather than
 * disappearing from it.
 *
 * Terminology follows `ui-strings.ts`, which was settled first and is the
 * authority per language. Product names, protocol names and the six SDK names
 * are not translated in any language.
 */

import type { Locale } from './locales'

/** Every locale except English, which reads its words from `docs.ts` itself. */
type Translated = Exclude<Locale, 'en'>

type Row = Record<Translated, string>

/** The section legends. These render through CSS uppercase. */
const LEGENDS: Record<string, Row> = {
  Start: {
    fr: 'Démarrer',
    es: 'Empezar',
    zh: '开始',
    ja: 'はじめる',
    ko: '시작',
    it: 'Iniziare',
    // Turkish uppercases i to a dotted I, so every legend here is a Turkish
    // word whose uppercase is correct. An English word left in this column
    // would render as BUİLD.
    tr: 'Başlarken',
    pt: 'Início',
  },
  Run: {
    fr: 'Exécuter',
    es: 'Ejecutar',
    zh: '运行',
    ja: '実行',
    ko: '실행',
    it: 'Eseguire',
    tr: 'Çalıştırma',
    pt: 'Executar',
  },
  Instrument: {
    fr: 'Instrumenter',
    es: 'Instrumentar',
    zh: '接入',
    ja: '計装',
    ko: '계측',
    it: 'Strumentare',
    tr: 'Ölçümleme',
    pt: 'Instrumentar',
  },
  Operate: {
    fr: 'Exploiter',
    es: 'Operar',
    zh: '运维',
    ja: '運用',
    ko: '운영',
    it: 'Gestire',
    tr: 'İşletme',
    pt: 'Operar',
  },
  Deploy: {
    fr: 'Déployer',
    es: 'Desplegar',
    zh: '部署',
    ja: 'デプロイ',
    ko: '배포',
    // Italian keeps the loanword: Distribuzione reads as a Linux
    // distribution beside Docker and Kubernetes.
    it: 'Deploy',
    tr: 'Dağıtım',
    pt: 'Implantação',
  },
  Reference: {
    fr: 'Référence',
    es: 'Referencia',
    zh: '参考',
    ja: 'リファレンス',
    ko: '참조',
    it: 'Riferimento',
    tr: 'Başvuru',
    pt: 'Referência',
  },
}

/** The entry titles, keyed by route. */
const TITLES: Record<string, Row> = {
  '/docs': {
    fr: "Vue d'ensemble",
    es: 'Vista general',
    zh: '概览',
    ja: '概要',
    ko: '개요',
    it: 'Panoramica',
    tr: 'Genel bakış',
    pt: 'Visão geral',
  },
  '/docs/quickstart': {
    fr: 'Prise en main',
    es: 'Inicio rápido',
    zh: '快速开始',
    ja: 'クイックスタート',
    ko: '빠른 시작',
    it: 'Avvio rapido',
    tr: 'Hızlı başlangıç',
    pt: 'Início rápido',
  },
  '/docs/install': {
    fr: 'Installation',
    es: 'Instalación',
    zh: '安装',
    ja: 'インストール',
    ko: '설치',
    it: 'Installazione',
    tr: 'Kurulum',
    pt: 'Instalação',
  },
  '/docs/concepts': {
    fr: 'Concepts',
    es: 'Conceptos',
    zh: '概念',
    ja: '概念',
    ko: '개념',
    it: 'Concetti',
    tr: 'Kavramlar',
    pt: 'Conceitos',
  },
  '/docs/from-mastra': {
    fr: 'Migrer depuis Mastra',
    es: 'Migrar desde Mastra',
    zh: '从 Mastra 迁移',
    ja: 'Mastra からの移行',
    ko: 'Mastra에서 이전',
    it: 'Passare da Mastra',
    tr: "Mastra'dan geçiş",
    pt: 'Migrando do Mastra',
  },
  '/docs/workflows': {
    fr: 'Workflows',
    es: 'Workflows',
    zh: '工作流',
    ja: 'ワークフロー',
    ko: '워크플로',
    // Italian does not pluralise loanwords.
    it: 'Workflow',
    tr: 'İş akışları',
    pt: 'Workflows',
  },
  '/docs/workers': {
    fr: 'Workers',
    es: 'Workers',
    zh: 'Worker',
    ja: 'ワーカー',
    ko: '워커',
    it: 'Worker',
    tr: "Worker'lar",
    pt: 'Workers',
  },
  '/docs/runs': {
    fr: 'Exécutions',
    es: 'Ejecuciones',
    zh: '运行',
    ja: '実行',
    ko: '실행',
    it: 'Esecuzioni',
    tr: 'Çalıştırmalar',
    pt: 'Execuções',
  },
  '/docs/integrating': {
    fr: 'Intégration',
    es: 'Integración',
    zh: '接入',
    ja: '導入',
    ko: '연동',
    it: 'Integrazione',
    tr: 'Entegrasyon',
    pt: 'Integração',
  },
  '/docs/opentelemetry': {
    fr: 'OpenTelemetry',
    es: 'OpenTelemetry',
    zh: 'OpenTelemetry',
    ja: 'OpenTelemetry',
    ko: 'OpenTelemetry',
    it: 'OpenTelemetry',
    tr: 'OpenTelemetry',
    pt: 'OpenTelemetry',
  },
  '/docs/sdks/typescript': {
    fr: 'TypeScript',
    es: 'TypeScript',
    zh: 'TypeScript',
    ja: 'TypeScript',
    ko: 'TypeScript',
    it: 'TypeScript',
    tr: 'TypeScript',
    pt: 'TypeScript',
  },
  '/docs/sdks/python': {
    fr: 'Python',
    es: 'Python',
    zh: 'Python',
    ja: 'Python',
    ko: 'Python',
    it: 'Python',
    tr: 'Python',
    pt: 'Python',
  },
  '/docs/sdks/rust': {
    fr: 'Rust',
    es: 'Rust',
    zh: 'Rust',
    ja: 'Rust',
    ko: 'Rust',
    it: 'Rust',
    tr: 'Rust',
    pt: 'Rust',
  },
  '/docs/sdks/go': {
    fr: 'Go',
    es: 'Go',
    zh: 'Go',
    ja: 'Go',
    ko: 'Go',
    it: 'Go',
    tr: 'Go',
    pt: 'Go',
  },
  '/docs/sdks/kotlin': {
    fr: 'Kotlin',
    es: 'Kotlin',
    zh: 'Kotlin',
    ja: 'Kotlin',
    ko: 'Kotlin',
    it: 'Kotlin',
    tr: 'Kotlin',
    pt: 'Kotlin',
  },
  '/docs/sdks/elixir': {
    fr: 'Elixir',
    es: 'Elixir',
    zh: 'Elixir',
    ja: 'Elixir',
    ko: 'Elixir',
    it: 'Elixir',
    tr: 'Elixir',
    pt: 'Elixir',
  },
  '/docs/searching': {
    fr: 'Retrouver une exécution',
    es: 'Encontrar una ejecución',
    zh: '查找运行',
    ja: '実行を探す',
    ko: '실행 찾기',
    it: "Trovare un'esecuzione",
    tr: 'Çalıştırma bulma',
    pt: 'Encontrar uma execução',
  },
  '/docs/cost': {
    fr: 'Comptabilité des coûts',
    es: 'Contabilidad de costos',
    zh: '成本核算',
    ja: 'コスト集計',
    ko: '비용 집계',
    it: 'Contabilità dei costi',
    tr: 'Maliyet muhasebesi',
    pt: 'Contabilidade de custos',
  },
  '/docs/evaluation': {
    fr: 'Scores et retours',
    es: 'Puntuaciones y comentarios',
    zh: '评分与反馈',
    ja: 'スコアとフィードバック',
    ko: '점수와 피드백',
    it: 'Punteggi e feedback',
    tr: 'Puanlar ve geri bildirim',
    pt: 'Pontuações e feedback',
  },
  /* `aspect` is the axis and `aspect note` is the sentence recorded against it,
     and the two nouns have to stay distinct in every language: a locale that
     translated both to one word would lose the distinction the whole family is
     built on. Japanese and Korean take 観点 and 관점, the word an evaluation
     rubric already uses for an axis, rather than transliterating "aspect". */
  '/docs/aspects': {
    fr: "Notes d'aspect",
    es: 'Notas de aspecto',
    zh: '方面注释',
    ja: '観点ノート',
    ko: '관점 노트',
    it: 'Note di aspetto',
    tr: 'Yön notları',
    pt: 'Notas de aspecto',
  },
  '/docs/themes': {
    fr: 'Thèmes et instantanés',
    es: 'Temas e instantáneas',
    zh: '主题与快照',
    ja: 'テーマとスナップショット',
    ko: '테마와 스냅샷',
    it: 'Temi e istantanee',
    tr: 'Temalar ve anlık görüntüler',
    pt: 'Temas e instantâneos',
  },
  '/docs/prompts': {
    fr: 'Prompts',
    es: 'Prompts',
    zh: '提示词',
    ja: 'プロンプト',
    ko: '프롬프트',
    it: 'Prompt',
    tr: 'İstemler',
    pt: 'Prompts',
  },
  '/docs/testing': {
    fr: 'Tests de régression',
    es: 'Pruebas de regresión',
    zh: '回归测试',
    ja: '回帰テスト',
    ko: '회귀 테스트',
    it: 'Test di regressione',
    tr: 'Regresyon testi',
    pt: 'Testes de regressão',
  },
  '/docs/alerting': {
    fr: 'Moniteurs et alertes',
    es: 'Monitores y alertas',
    zh: '监控与告警',
    ja: 'モニターとアラート',
    ko: '모니터와 알림',
    it: 'Monitor e avvisi',
    tr: 'İzleyiciler ve uyarılar',
    pt: 'Monitores e alertas',
  },
  '/docs/privacy': {
    fr: 'Masquage et conservation',
    es: 'Ocultación y retención',
    zh: '脱敏与保留',
    ja: 'マスキングと保持',
    ko: '마스킹과 보존',
    it: 'Oscuramento e conservazione',
    tr: 'Maskeleme ve saklama',
    pt: 'Ocultação e retenção',
  },
  '/docs/sampling': {
    fr: 'Échantillonnage',
    es: 'Muestreo',
    zh: '采样',
    ja: 'サンプリング',
    ko: '샘플링',
    it: 'Campionamento',
    tr: 'Örnekleme',
    pt: 'Amostragem',
  },
  '/deploy': {
    fr: 'Docker et Kubernetes',
    es: 'Docker y Kubernetes',
    zh: 'Docker 与 Kubernetes',
    ja: 'Docker と Kubernetes',
    ko: 'Docker와 Kubernetes',
    it: 'Docker e Kubernetes',
    tr: 'Docker ve Kubernetes',
    pt: 'Docker e Kubernetes',
  },
  '/docs/configuration': {
    fr: 'Configuration',
    es: 'Configuración',
    zh: '配置',
    ja: '設定',
    ko: '설정',
    it: 'Configurazione',
    tr: 'Yapılandırma',
    pt: 'Configuração',
  },
  '/docs/accounts': {
    fr: 'Comptes et organisations',
    es: 'Cuentas y organizaciones',
    zh: '账户与组织',
    ja: 'アカウントと組織',
    ko: '계정과 조직',
    it: 'Account e organizzazioni',
    tr: 'Hesaplar ve kuruluşlar',
    pt: 'Contas e organizações',
  },
  '/docs/operations': {
    fr: 'Exploitation',
    es: 'Operación',
    zh: '运维',
    ja: '運用',
    ko: '운영',
    it: 'Gestione',
    tr: 'İşletme',
    pt: 'Operação',
  },
  '/docs/forwarding': {
    fr: 'Transfert',
    es: 'Reenvío',
    zh: '转发',
    ja: '転送',
    ko: '전달',
    it: 'Inoltro',
    tr: 'İletme',
    pt: 'Encaminhamento',
  },
  '/docs/relay': {
    fr: 'Relais et régions',
    es: 'Relé y regiones',
    zh: '中继与多区域',
    ja: 'リレーとリージョン',
    ko: '릴레이와 리전',
    it: 'Relay e regioni',
    tr: 'Aktarıcı ve bölgeler',
    pt: 'Relay e regiões',
  },
  '/docs/security': {
    fr: 'Sécurité',
    es: 'Seguridad',
    zh: '安全',
    ja: 'セキュリティ',
    ko: '보안',
    it: 'Sicurezza',
    tr: 'Güvenlik',
    pt: 'Segurança',
  },
  '/docs/reference/data-model': {
    fr: 'Modèle de données',
    es: 'Modelo de datos',
    zh: '数据模型',
    ja: 'データモデル',
    ko: '데이터 모델',
    it: 'Modello dei dati',
    tr: 'Veri modeli',
    pt: 'Modelo de dados',
  },
  '/docs/reference/api': {
    fr: 'API HTTP',
    es: 'API HTTP',
    zh: 'HTTP API',
    ja: 'HTTP API',
    ko: 'HTTP API',
    it: 'API HTTP',
    tr: 'HTTP API',
    pt: 'API HTTP',
  },
}

/**
 * The title for a route in a locale.
 *
 * Falls back to the English title rather than to nothing, so an entry added to
 * `docs.ts` without a translation still names itself in a translated sidebar.
 */
export function docTitle(href: string, english: string, locale: Locale): string {
  if (locale === 'en') return english
  return TITLES[href]?.[locale] ?? english
}

/** The legend for a section in a locale, falling back the same way. */
export function docLegend(english: string, locale: Locale): string {
  if (locale === 'en') return english
  return LEGENDS[english]?.[locale] ?? english
}

/**
 * Every route or legend that has no translation for a locale.
 *
 * Read by the consistency gate, so a page added to `docs.ts` and never
 * translated is reported rather than discovered by a reader.
 */
export function missingDocStrings(routes: string[], legends: string[]): string[] {
  const gaps: string[] = []
  for (const r of routes) {
    if (!TITLES[r]) gaps.push(`title ${r}`)
  }
  for (const l of legends) {
    if (!LEGENDS[l]) gaps.push(`legend ${l}`)
  }
  return gaps
}
