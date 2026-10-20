// The version as the app shows it: with the release stage from package.json ("beta") after the
// number, wherever the number appears. Drop `releaseStage` there once the app leaves beta.
import pkg from "../package.json";

const STAGE: string = (pkg as { releaseStage?: string }).releaseStage ?? "";

/** "0.2.1 beta", or just "0.2.1" without a stage. */
export function versionLabel(version: string): string {
  return STAGE ? `${version} ${STAGE}` : version;
}
