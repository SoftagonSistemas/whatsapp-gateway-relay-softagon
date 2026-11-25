/**
 * Phone number detection and formatting for the Raycast-style command bar.
 * Rules from PROMPT.md:
 * - Only digits, spaces, and `+()-.`
 * - 10 or more digits
 * - Automatically prepends `55` if missing (for 10 or 11 digit Brazilian numbers)
 */
export function parsePhoneInput(raw: string): {
  isPhone: boolean;
  isJid: boolean;
  digits: string;
  formatted: string;
} {
  const trimmed = raw.trim();
  if (!trimmed) return { isPhone: false, isJid: false, digits: '', formatted: '' };

  if (trimmed.includes('@')) {
    const validJid = /^[a-zA-Z0-9._:-]+@(s\.whatsapp\.net|g\.us|lid)$/.test(trimmed);
    return {
      isPhone: false,
      isJid: validJid,
      digits: validJid ? trimmed : '',
      formatted: validJid ? trimmed : '',
    };
  }

  if (!/^[\d\s+().-]+$/.test(trimmed)) {
    return { isPhone: false, isJid: false, digits: '', formatted: '' };
  }

  const hasExplicitPlus = trimmed.startsWith('+');
  let digits = trimmed.replace(/\D/g, '');
  if (digits.length < 8 || digits.length > 15) {
    return { isPhone: false, isJid: false, digits: '', formatted: '' };
  }

  // Only auto-prepend Brazil DDI (55) when the user typed a 10 or 11-digit local number without '+'
  if (!hasExplicitPlus && (digits.length === 10 || digits.length === 11) && !digits.startsWith('55')) {
    digits = `55${digits}`;
  }

  // Require at least 10 digits total for a valid international phone (or 8+ if explicit '+')
  if (!hasExplicitPlus && digits.length < 10) {
    return { isPhone: false, isJid: false, digits: '', formatted: '' };
  }

  return {
    isPhone: true,
    isJid: false,
    digits,
    formatted: formatPhoneDigits(digits),
  };
}

export function formatPhoneDigits(digits: string): string {
  const d = digits.replace(/\D/g, '');
  if (d.startsWith('55') && d.length === 13) {
    return `+55 (${d.slice(2, 4)}) ${d.slice(4, 9)}-${d.slice(9)}`;
  }
  if (d.startsWith('55') && d.length === 12) {
    return `+55 (${d.slice(2, 4)}) ${d.slice(4, 8)}-${d.slice(8)}`;
  }
  if (d.length >= 10) {
    return `+${d}`;
  }
  return d;
}

export function normalizeText(str: string): string {
  return str
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase();
}
