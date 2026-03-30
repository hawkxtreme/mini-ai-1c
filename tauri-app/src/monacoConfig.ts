/**
 * Configures Monaco Editor to use locally bundled files instead of CDN.
 * This prevents "Loading..." hanging on RDP sessions or restricted networks
 * where jsdelivr.net CDN may be slow or blocked.
 *
 * Must be imported before any @monaco-editor/react usage.
 */
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import { loader } from '@monaco-editor/react';

(self as any).MonacoEnvironment = {
    getWorker: () => new editorWorker(),
};

loader.config({ monaco });
