const cjkRegex =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

function findFirstMatch(text, regex) {
  if (!text) return null;
  const match = text.match(regex);
  return match ? match[0] : null;
}

function isAsciiUppercase(char) {
  return typeof char === "string" && char.length > 0 && char >= "A" && char <= "Z";
}

module.exports = {
  extends: ["@commitlint/config-conventional"],
  plugins: ["commitlint-plugin-function-rules"],
  rules: {
    "type-enum": [
      2,
      "always",
      ["build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test"],
    ],

    "subject-case": [0],
    "body-case": [0],

    "function-rules/subject-case": [
      2,
      "always",
      (parsed) => {
        const subject = parsed.subject ?? "";

        const bad = findFirstMatch(subject, cjkRegex);
        if (bad) {
          return [false, `Subject must not contain CJK characters (found: ${JSON.stringify(bad)})`];
        }

        const first = subject.trim().charAt(0);
        if (isAsciiUppercase(first)) {
          return [false, "Subject must not start with an uppercase letter"];
        }

        return [true];
      },
    ],
    "function-rules/body-case": [
      2,
      "always",
      (parsed) => {
        const body = parsed.body ?? "";
        const footer = parsed.footer ?? "";
        const text = [body, footer].filter(Boolean).join("\n");

        const bad = findFirstMatch(text, cjkRegex);
        if (bad) {
          return [false, `Body must not contain CJK characters (found: ${JSON.stringify(bad)})`];
        }

        return [true];
      },
    ],
  },
};
