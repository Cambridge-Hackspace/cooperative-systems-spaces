/**
 * Leave the app for another site (#225): an OAuth redirect back to the app
 * that asked CS:S to sign a member in, or wherever a logout goes next.
 *
 * Its own module so a test can stand in for it: jsdom can't follow a real
 * navigation, and asserting where the browser was sent is the whole point.
 */
export function leaveFor(url: string): void {
  window.location.assign(url)
}
