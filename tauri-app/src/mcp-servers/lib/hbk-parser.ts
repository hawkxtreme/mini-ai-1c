/**
 * HBK Parser — Нативный TypeScript парсер файлов справки 1С:Предприятие (.hbk)
 *
 * Реальная архитектура формата (исследована на shcntx_ru.hbk платформы 8.3.27.1989):
 *
 * Контейнер HBK:
 *   [Header 16 bytes] firstFreeBlock(4) + defaultBlockSize(4) + unknown(8)
 *   [Блоки]: каждый начинается с ASCII-заголовка вида:
 *     CRLF + payloadSize(8 hex) + SPACE + blockSize(8 hex) + SPACE + nextBlock(8 hex) + SPACE+CRLF
 *     = 31 байт заголовок блока
 *
 * TOC (первый блок, offset=16):
 *   7 записей FileInfo по 12 байт (headerAddr 4b, bodyAddr 4b, reserved 4b)
 *   Адреса — ПРЯМЫЕ БАЙТОВЫЕ СМЕЩЕНИЯ в файле (не индексы блоков!)
 *
 * Файлы верхнего уровня:
 *   - Book: JSON конфигурация
 *   - FileStorage: ZIP архив с HTML страницами справки (52000+ файлов)
 *   - IndexMainData, IndexPackBlock, MainData, PackBlock, PackLookup: индексы
 *
 * Извлечение HTML: читаем FileStorage blob → парсим как ZIP → inflateRaw каждый .html файл
 */

import { readFileSync } from 'fs';
import { inflateRawSync } from 'zlib';

/** Результат парсинга одной страницы справки */
export interface HbkPage {
    /** Имя файла внутри ZIP (путь относительно корня ZIP) */
    name: string;
    /** HTML содержимое страницы (windows-1251 или utf-8, обычно короткий html) */
    html: string;
}

/** Callback прогресса: (parsed, total) */
export type ProgressCallback = (parsed: number, total: number) => void;

// ─────────────────────── Внутренние утилиты ───────────────────────────────

/**
 * Читает заголовок блока по прямому байтовому смещению.
 * Возвращает размер полезной нагрузки, начало данных и следующий блок.
 */
function readBlock(buf: Buffer, rawOffset: number): {
    payloadSize: number;
    blockSize: number;
    nextRaw: number | null;
    dataStart: number;
} {
    let p = rawOffset + 2; // skip CRLF
    const payloadSize = parseInt(buf.subarray(p, p + 8).toString('ascii'), 16);
    p += 9; // hex + SPACE
    const blockSize = parseInt(buf.subarray(p, p + 8).toString('ascii'), 16);
    p += 9; // hex + SPACE
    const nextBlockHex = parseInt(buf.subarray(p, p + 8).toString('ascii'), 16);
    p += 11; // hex + SPACE + CRLF

    return {
        payloadSize,
        blockSize,
        nextRaw: nextBlockHex === 0x7fffffff ? null : nextBlockHex,
        dataStart: p,
    };
}

/**
 * Читает всю цепочку блоков начиная с rawOffset, конкатенирует данные.
 * Цепочка: nextRaw указывает прямой байтовый адрес следующего блока или null.
 */
function readEntityFull(buf: Buffer, rawOffset: number): Buffer {
    const chunks: Buffer[] = [];
    let off: number | null = rawOffset;

    while (off !== null) {
        const blk = readBlock(buf, off);
        chunks.push(buf.subarray(blk.dataStart, blk.dataStart + blk.payloadSize));
        off = blk.nextRaw;
    }

    return Buffer.concat(chunks);
}

/**
 * Читает TOC (таблицу оглавления) из первого блока HBK.
 * Возвращает массив { headerAddr, bodyAddr } с прямыми байтовыми смещениями.
 */
function parseTOC(buf: Buffer): Array<{ headerAddr: number; bodyAddr: number }> {
    const blk = readBlock(buf, 16);
    const tocData = buf.subarray(blk.dataStart, blk.dataStart + blk.payloadSize);
    const count = Math.floor(tocData.length / 12);
    const entries: Array<{ headerAddr: number; bodyAddr: number }> = [];

    for (let i = 0; i < count; i++) {
        const base = i * 12;
        const headerAddr = tocData.readInt32LE(base);
        const bodyAddr = tocData.readInt32LE(base + 4);
        entries.push({ headerAddr, bodyAddr });
    }

    return entries;
}

/**
 * Читает имя файла из header-блока.
 * DATA layout: 8+8+4=20 байт доп. полей, затем имя в UTF-16LE.
 */
function readFileName(buf: Buffer, headerRaw: number): string {
    const blk = readBlock(buf, headerRaw);
    const nameLen = blk.payloadSize - 20;
    if (nameLen <= 0) return '';
    return buf
        .subarray(blk.dataStart + 20, blk.dataStart + 20 + nameLen)
        .toString('utf16le')
        .replace(/\0/g, '');
}

// ─────────────────────── ZIP распаковка ───────────────────────────────────

/** ZIP local file header magic */
const ZIP_LFH_SIG = 0x04034b50;

interface ZipEntry {
    name: string;
    compressedData: Buffer;
    compMethod: number;
    uncompSize: number;
}

/**
 * Парсит ZIP-буфер и возвращает итерируемую последовательность файлов.
 * Поддерживает только deflate (method=8) и stored (method=0).
 */
function* iterZipEntries(zipBuf: Buffer): Generator<ZipEntry> {
    let pos = 0;
    while (pos < zipBuf.length - 30) {
        const sig = zipBuf.readUInt32LE(pos);
        if (sig !== ZIP_LFH_SIG) break;

        const compMethod = zipBuf.readUInt16LE(pos + 8);
        const compSize = zipBuf.readUInt32LE(pos + 18);
        const uncompSize = zipBuf.readUInt32LE(pos + 22);
        const nameLen = zipBuf.readUInt16LE(pos + 26);
        const extraLen = zipBuf.readUInt16LE(pos + 28);

        const name = zipBuf.subarray(pos + 30, pos + 30 + nameLen).toString('utf8');
        const dataStart = pos + 30 + nameLen + extraLen;
        const compressedData = zipBuf.subarray(dataStart, dataStart + compSize);

        pos = dataStart + compSize;

        yield { name, compressedData, compMethod, uncompSize };
    }
}

// ─────────────────────── Публичный API ────────────────────────────────────

/**
 * Возвращает примерное количество HTML файлов в HBK.
 * Читает только TOC и размер FileStorage для оценки.
 */
export function getHbkFileCount(filePath: string): number {
    try {
        const buf = readFileSync(filePath);
        const toc = parseTOC(buf);
        // FileStorage — обычно второй элемент (index 1)
        if (toc.length < 2) return 0;
        const fsBody = readEntityFull(buf, toc[1].bodyAddr);
        // Быстрая оценка: ~740 байт на запись ZIP в среднем (заголовок + данные)
        return Math.floor(fsBody.length / 740);
    } catch {
        return 0;
    }
}

/**
 * Основная функция: итерирует HTML страницы из HBK файла.
 * Читает FileStorage (ZIP) и извлекает все .html файлы.
 *
 * @param filePath Путь к .hbk файлу
 * @param onProgress Callback прогресса (опционально)
 * @returns AsyncIterable<HbkPage>
 */
export async function* parseHbk(
    filePath: string,
    onProgress?: ProgressCallback
): AsyncGenerator<HbkPage> {
    const buf = readFileSync(filePath);
    const toc = parseTOC(buf);

    // Ищем FileStorage (обычно index 1, но на всякий случай ищем по имени)
    let fsBodyAddr: number | null = null;
    for (const entry of toc) {
        try {
            const name = readFileName(buf, entry.headerAddr);
            if (name.toLowerCase().includes('filestorage')) {
                fsBodyAddr = entry.bodyAddr;
                break;
            }
        } catch {
            // ignore
        }
    }

    // Fallback: второй элемент TOC
    if (fsBodyAddr === null && toc.length >= 2) {
        fsBodyAddr = toc[1].bodyAddr;
    }

    if (fsBodyAddr === null) return;

    // Читаем весь FileStorage (может быть в нескольких блоках)
    const zipBuf = readEntityFull(buf, fsBodyAddr);

    if (zipBuf.length < 4 || zipBuf.readUInt32LE(0) !== ZIP_LFH_SIG) {
        process.stderr.write(`[hbk-parser] FileStorage is not a ZIP archive in ${filePath}\n`);
        return;
    }

    // Считаем всего .html файлов для прогресса
    let total = 0;
    for (const entry of iterZipEntries(zipBuf)) {
        if (entry.name.endsWith('.html')) total++;
    }

    let parsed = 0;

    for (const entry of iterZipEntries(zipBuf)) {
        if (!entry.name.endsWith('.html')) continue;

        try {
            let html: string;
            if (entry.compMethod === 0) {
                // stored
                html = entry.compressedData.toString('utf8');
            } else if (entry.compMethod === 8) {
                // deflate
                const raw = inflateRawSync(entry.compressedData);
                html = raw.toString('utf8');
            } else {
                continue;
            }

            parsed++;
            onProgress?.(parsed, total);

            yield { name: entry.name, html };
        } catch {
            // Пропускаем повреждённые записи
        }

        // Даём event loop отдышаться каждые 100 страниц
        if (parsed % 100 === 0) {
            await new Promise(r => setImmediate(r));
        }
    }
}
