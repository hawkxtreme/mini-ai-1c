/**
 * 1С:Справка — MCP Сервер
 *
 * Предоставляет ИИ инструменты для поиска по официальной справке платформы 1С:Предприятие 8.3.
 * Читает .hbk файлы напрямую через нативный TypeScript парсер (без Java/JAR).
 *
 * Статусы (передаются через stderr для отображения в UI):
 *   HELP_STATUS:unavailable  — платформа 1С не найдена
 *   HELP_STATUS:indexing:N:TOTAL:msg — идёт индексация
 *   HELP_STATUS:ready:VERSION:COUNT  — готов к работе
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { existsSync, mkdirSync, statSync, readdirSync } from 'fs';
import { join } from 'path';
import { load as parseHtml } from 'cheerio';
import Database from 'better-sqlite3';
import { parseHbk } from './lib/hbk-parser.js';
import { tmpdir, homedir } from 'os';

// ---------- Утилиты ----------

function reportStatus(status: string) {
    process.stderr.write(`HELP_STATUS:${status}\n`);
}

function log(msg: string) {
    process.stderr.write(`[1c-help] ${msg}\n`);
}

// ---------- Поиск платформы 1С ----------

interface PlatformInfo {
    version: string;
    binPath: string;
}

/**
 * Ищет установленные версии платформы 1С в стандартных путях.
 * Возвращает последнюю по семантической версии.
 */
function findPlatform(): PlatformInfo | null {
    const { platform } = process;

    const searchPaths = platform === 'win32'
        ? [
            'C:\\Program Files\\1cv8',
            'C:\\Program Files (x86)\\1cv8',
        ]
        : [
            '/opt/1cv8',
            '/opt/1cv8/x86_64',
            '/usr/share/1cv8',
        ];

    const platforms: PlatformInfo[] = [];

    for (const basePath of searchPaths) {
        if (!existsSync(basePath)) continue;

        let entries: string[] = [];
        try {
            entries = readdirSync(basePath);
        } catch {
            continue;
        }

        for (const entry of entries) {
            // Версия платформы — папка типа "8.3.27.1989"
            if (!/^\d+\.\d+\.\d+\.\d+$/.test(entry)) continue;

            const binPath = join(basePath, entry, 'bin');
            if (!existsSync(binPath)) continue;

            // Проверяем наличие нужного .hbk файла
            const hbkPath = join(binPath, 'shcntx_ru.hbk');
            if (!existsSync(hbkPath)) continue;

            platforms.push({ version: entry, binPath });
        }
    }

    if (platforms.length === 0) return null;

    // Сортируем по семантической версии — берём последнюю
    platforms.sort((a, b) => {
        const partsA = a.version.split('.').map(Number);
        const partsB = b.version.split('.').map(Number);
        for (let i = 0; i < 4; i++) {
            if ((partsA[i] || 0) !== (partsB[i] || 0)) {
                return (partsB[i] || 0) - (partsA[i] || 0);
            }
        }
        return 0;
    });

    return platforms[0];
}

// ---------- Путь к базе данных ----------

function getDbPath(): string {
    // Используем AppData/home директорию пользователя для хранения базы
    const base = process.env.APPDATA || process.env.HOME || homedir() || tmpdir();
    const dir = join(base, 'com.mini-ai-1c', 'help');
    if (!existsSync(dir)) {
        mkdirSync(dir, { recursive: true });
    }
    return join(dir, 'help.db');
}

// ---------- Инициализация SQLite ----------

function initDatabase(dbPath: string): Database.Database {
    const db = new Database(dbPath);
    db.pragma('journal_mode = WAL');
    db.pragma('synchronous = NORMAL');

    db.exec(`
    CREATE TABLE IF NOT EXISTS meta (
      key TEXT PRIMARY KEY,
      value TEXT
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS topics USING fts5(
      topic_id,
      title,
      content,
      category,
      version,
      tokenize = "unicode61"
    );
  `);

    return db;
}

// ---------- Индексация ----------

const HBK_FILES = [
    { file: 'shcntx_ru.hbk', category: 'syntax' },
    { file: 'shquery_ru.hbk', category: 'query' },
    { file: 'shlang_ru.hbk', category: 'language' },
];

/**
 * Извлекает текст из HTML — убирает теги, оставляет читаемый текст.
 */
function extractText(html: string): { title: string; text: string } {
    const $ = parseHtml(html);

    // Заголовок страницы
    const title = $('h1, h2, title').first().text().trim() || 'Без названия';

    // Убираем навигационные элементы и скрипты
    $('script, style, nav, .toc, .navigation').remove();

    // Получаем чистый текст
    const text = $('body').text()
        .replace(/\s+/g, ' ')
        .trim()
        .substring(0, 10000); // Ограничиваем размер

    return { title, text };
}

/**
 * Запускает полную индексацию всех HBK файлов в фоне.
 */
async function runIndexing(platform: PlatformInfo, db: Database.Database): Promise<void> {
    const version = platform.version;

    // Удаляем старые данные этой версии
    db.prepare("DELETE FROM topics WHERE version = ?").run(version);

    const insertStmt = db.prepare(`
    INSERT INTO topics (topic_id, title, content, category, version)
    VALUES (?, ?, ?, ?, ?)
  `);

    let totalProcessed = 0;
    let totalFiles = 0;

    // Считаем примерное кол-во страниц (берём из первого файла для оценки)
    const mainHbkPath = join(platform.binPath, 'shcntx_ru.hbk');
    // Грубая оценка: 1 файл на каждые ~35 KiB
    try {
        const size = statSync(mainHbkPath).size;
        totalFiles = Math.floor(size / 35000);
    } catch {
        totalFiles = 1000;
    }

    for (const hbkDef of HBK_FILES) {
        const hbkPath = join(platform.binPath, hbkDef.file);
        if (!existsSync(hbkPath)) continue;

        log(`Индексируется: ${hbkDef.file}`);

        const insertMany = db.transaction((pages: Array<[string, string, string, string, string]>) => {
            for (const page of pages) {
                insertStmt.run(...page);
            }
        });

        let batch: Array<[string, string, string, string, string]> = [];

        for await (const page of parseHbk(hbkPath, (done, total) => {
            totalProcessed++;
            const progress = Math.min(99, Math.floor((totalProcessed / Math.max(totalFiles, 1)) * 100));
            reportStatus(`indexing:${progress}:${totalFiles}:Обработано ${totalProcessed} страниц...`);
        })) {
            const { title, text } = extractText(page.html);
            const topicId = `${version}/${hbkDef.category}/${page.name}`;

            batch.push([topicId, title, text, hbkDef.category, version]);

            // Записываем батчами по 100 страниц
            if (batch.length >= 100) {
                insertMany(batch);
                batch = [];
            }
        }

        if (batch.length > 0) {
            insertMany(batch);
        }
    }

    // Сохраняем метаданные
    const count = (db.prepare("SELECT COUNT(*) as c FROM topics WHERE version = ?").get(version) as any).c;
    db.prepare("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)").run('indexed_version', version);
    db.prepare("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)").run('topic_count', String(count));
    db.prepare("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)").run('indexed_at', new Date().toISOString());

    reportStatus(`ready:${version}:${count}`);
    log(`Индексация завершена. Всего тем: ${count}`);
}

// ---------- MCP Сервер ----------

const server = new Server(
    { name: '1c-help', version: '1.0.0' },
    { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: [
        {
            name: 'search_1c_help',
            description: 'Полнотекстовый поиск по официальной справке платформы 1С:Предприятие 8.3. ' +
                'Ищет по всем разделам: встроенный язык, объектная модель, язык запросов. ' +
                'Используй для поиска методов, свойств, операторов, функций встроенного языка.',
            inputSchema: {
                type: 'object',
                properties: {
                    query: {
                        type: 'string',
                        description: 'Поисковый запрос (название метода, объекта, функции или описание задачи)',
                    },
                    limit: {
                        type: 'number',
                        description: 'Максимальное количество результатов (по умолчанию 5)',
                    },
                    category: {
                        type: 'string',
                        enum: ['syntax', 'query', 'language', 'all'],
                        description: 'Раздел справки: syntax — объектная модель, query — язык запросов, language — встроенный язык',
                    },
                },
                required: ['query'],
            },
        },
        {
            name: 'get_1c_help_topic',
            description: 'Получить полное содержимое темы из справки 1С по её идентификатору. ' +
                'Используй topic_id из результатов search_1c_help.',
            inputSchema: {
                type: 'object',
                properties: {
                    topic_id: {
                        type: 'string',
                        description: 'Идентификатор темы из результатов поиска',
                    },
                },
                required: ['topic_id'],
            },
        },
        {
            name: 'list_1c_help_versions',
            description: 'Получить список проиндексированных версий платформы 1С и статистику.',
            inputSchema: {
                type: 'object',
                properties: {},
                required: [],
            },
        },
        {
            name: 'reindex_1c_help',
            description: 'Принудительно пересоздать индекс справки 1С:Предприятие. ' +
                'Используй если база данных справки пустая или устаревшая.',
            inputSchema: {
                type: 'object',
                properties: {},
                required: [],
            },
        },
    ],
}));

// ---------- Обработчики инструментов ----------

let db: Database.Database | null = null;
let isIndexing = false;
let currentPlatform: ReturnType<typeof findPlatform> = null;

server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;

    // Если БД ещё не готова
    if (!db) {
        return {
            content: [{
                type: 'text',
                text: isIndexing
                    ? '⏳ База данных справки 1С подготавливается (индексация).\n' +
                    'Пожалуйста, подождите 1-3 минуты и повторите запрос.'
                    : '⚠️ База данных справки 1С недоступна.',
            }],
        };
    }

    switch (name) {
        case 'search_1c_help': {
            const query = String(args?.query || '').trim();
            const limit = Number(args?.limit || 5);
            const category = String(args?.category || 'all');

            if (!query) {
                return { content: [{ type: 'text', text: 'Ошибка: укажите поисковый запрос.' }] };
            }

            // FTS5 запрос с учётом категории
            let sql = 'SELECT topic_id, title, snippet(topics, 2, ">>", "<<", "...", 30) as excerpt FROM topics WHERE topics MATCH ? ORDER BY rank LIMIT ?';
            let params: any[] = [query, limit];

            if (category !== 'all') {
                sql = 'SELECT topic_id, title, snippet(topics, 2, ">>", "<<", "...", 30) as excerpt FROM topics WHERE topics MATCH ? AND category = ? ORDER BY rank LIMIT ?';
                params = [query, category, limit];
            }

            let results: any[] = [];
            try {
                results = db.prepare(sql).all(...params) as any[];
            } catch {
                // Если FTS запрос упал — пробуем LIKE (более толерантный)
                results = db.prepare(
                    `SELECT topic_id, title, substr(content, 1, 300) as excerpt FROM topics WHERE title LIKE ? OR content LIKE ? LIMIT ?`
                ).all(`%${query}%`, `%${query}%`, limit) as any[];
            }

            if (results.length === 0) {
                return { content: [{ type: 'text', text: `По запросу "${query}" ничего не найдено в справке 1С.` }] };
            }

            const text = results.map((r, i) =>
                `**${i + 1}. ${r.title}**\n` +
                `ID: \`${r.topic_id}\`\n` +
                `${r.excerpt}\n`
            ).join('\n---\n');

            return { content: [{ type: 'text', text: `## Результаты поиска по справке 1С: "${query}"\n\n${text}` }] };
        }

        case 'get_1c_help_topic': {
            const topicId = String(args?.topic_id || '').trim();
            if (!topicId) {
                return { content: [{ type: 'text', text: 'Ошибка: укажите topic_id.' }] };
            }

            const row = db.prepare('SELECT title, content FROM topics WHERE topic_id = ?').get(topicId) as any;
            if (!row) {
                return { content: [{ type: 'text', text: `Тема "${topicId}" не найдена.` }] };
            }

            return {
                content: [{
                    type: 'text',
                    text: `# ${row.title}\n\n${row.content}`,
                }],
            };
        }

        case 'list_1c_help_versions': {
            const version = (db.prepare("SELECT value FROM meta WHERE key = 'indexed_version'").get() as any)?.value;
            const count = (db.prepare("SELECT value FROM meta WHERE key = 'topic_count'").get() as any)?.value;
            const indexedAt = (db.prepare("SELECT value FROM meta WHERE key = 'indexed_at'").get() as any)?.value;

            return {
                content: [{
                    type: 'text',
                    text: version
                        ? `## 1С:Справка — Статус\n\n✅ Готово\n- Версия платформы: **${version}**\n- Тем в базе: **${count}**\n- Дата индексации: ${indexedAt}`
                        : '⚠️ База данных не содержит проиндексированных версий.',
                }],
            };
        }

        case 'reindex_1c_help': {
            if (isIndexing) {
                return { content: [{ type: 'text', text: '⏳ Индексация уже выполняется. Подождите завершения.' }] };
            }
            if (!currentPlatform) {
                return { content: [{ type: 'text', text: '⚠️ Платформа 1С не найдена. Переиндексация невозможна.' }] };
            }
            // Сбрасываем метаданные чтобы принудить переиндексацию
            if (db) {
                try {
                    db.prepare("DELETE FROM meta").run();
                    db.prepare("DELETE FROM topics").run();
                } catch { /* ignore */ }
            } else {
                db = initDatabase(getDbPath());
            }
            isIndexing = true;
            reportStatus(`indexing:0:1000:Запуск переиндексации...`);
            runIndexing(currentPlatform, db)
                .then(() => { isIndexing = false; })
                .catch((err) => {
                    isIndexing = false;
                    log(`Ошибка переиндексации: ${err.message}`);
                    reportStatus('unavailable:Reindex failed');
                });
            return { content: [{ type: 'text', text: '🔄 Переиндексация запущена. Займёт 1-3 минуты.' }] };
        }

        default:
            return { content: [{ type: 'text', text: `Неизвестный инструмент: ${name}` }] };
    }
});

// ---------- Точка входа ----------

async function main() {
    // 1. Ищем платформу 1С
    const platform = findPlatform();

    if (!platform) {
        reportStatus('unavailable:1C Platform not found in standard paths');
        log('Платформа 1С не найдена. Проверьте установку 1С:Предприятие 8.3.');
        // Запускаем MCP сервер в «спящем» режиме — инструменты сообщат о проблеме
        const transport = new StdioServerTransport();
        await server.connect(transport);
        return;
    }

    log(`Найдена платформа: ${platform.version} (${platform.binPath})`);
    currentPlatform = platform;

    // 2. Инициализируем/открываем базу данных
    const dbPath = getDbPath();
    const dbExists = existsSync(dbPath);

    // Определяем нужна ли переиндексация
    let needsIndexing = !dbExists;

    if (dbExists) {
        try {
            const tempDb = new Database(dbPath, { readonly: true });
            const indexedVersion = (tempDb.prepare("SELECT value FROM meta WHERE key = 'indexed_version'").get() as any)?.value;
            const topicCount = parseInt((tempDb.prepare("SELECT value FROM meta WHERE key = 'topic_count'").get() as any)?.value || '0', 10);
            tempDb.close();

            // Переиндексируем если версия изменилась ИЛИ база пустая (сломанная прошлая индексация)
            if (indexedVersion !== platform.version) {
                log(`Версия изменилась: ${indexedVersion} → ${platform.version}. Требуется переиндексация.`);
                needsIndexing = true;
            } else if (topicCount === 0) {
                log(`База данных пустая (0 тем). Требуется переиндексация.`);
                needsIndexing = true;
            } else {
                // База актуальна — открываем в нормальном режиме
                db = initDatabase(dbPath);
                reportStatus(`ready:${platform.version}:${topicCount}`);
                log(`База данных готова: ${topicCount} тем.`);
            }
        } catch {
            needsIndexing = true;
        }
    }

    // 3. Если нужна индексация — запускаем в фоне
    if (needsIndexing) {
        db = initDatabase(dbPath);
        isIndexing = true;
        reportStatus(`indexing:0:1000:Запуск индексации...`);

        // Запускаем индексацию асинхронно
        runIndexing(platform, db)
            .then(() => {
                isIndexing = false;
            })
            .catch((err) => {
                isIndexing = false;
                log(`Ошибка индексации: ${err.message}`);
                reportStatus('unavailable:Indexing failed');
            });
    }

    // 4. Запускаем MCP сервер (обрабатывает запросы параллельно с индексацией)
    const transport = new StdioServerTransport();
    await server.connect(transport);
}

main().catch((err) => {
    log(`Критическая ошибка: ${err.message}`);
    process.exit(1);
});
