import ReactReconciler from 'react-reconciler';
import { DefaultEventPriority } from 'react-reconciler/constants.js';

/**
 * What the reconciler builds: the JSX tree, resolved.
 *
 * Components have been called, hooks have run, conditionals have been decided.
 * What is left is host elements with plain props — one step from the IR, and
 * the step is taken in `document.ts`.
 */
export interface Instance {
  type: string;
  props: Record<string, unknown>;
  children: Node[];
}

export interface TextInstance {
  type: '#text';
  text: string;
}

export type Node = Instance | TextInstance;

/**
 * Text or an element.
 *
 * A hand-written guard because `Instance.type` is any string and TypeScript
 * cannot tell the two apart from `type === '#text'` alone.
 */
export function isText(node: Node): node is TextInstance {
  return node.type === '#text';
}

export interface Container {
  children: Node[];
}

const NO_TIMEOUT = -1;

let currentPriority = DefaultEventPriority;

const HOST_CONTEXT = {};

/**
 * A host config for a target that is read once and never touched again.
 *
 * There is no screen, nothing to update and nobody to click: `render` builds a
 * tree, hands it over and drops it. The mutation methods still have to exist —
 * React calls them while it builds — but nothing here needs to be efficient
 * about changes, because there are none.
 */
const config = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: true,
  noTimeout: NO_TIMEOUT,
  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,

  createInstance(type: string, props: Record<string, unknown>): Instance {
    return { type, props, children: [] };
  },
  createTextInstance(text: string): TextInstance {
    return { type: '#text', text };
  },

  appendInitialChild(parent: Instance, child: Node) {
    parent.children.push(child);
  },
  appendChild(parent: Instance, child: Node) {
    parent.children.push(child);
  },
  appendChildToContainer(container: Container, child: Node) {
    container.children.push(child);
  },
  insertBefore(parent: Instance, child: Node, before: Node) {
    parent.children.splice(parent.children.indexOf(before), 0, child);
  },
  insertInContainerBefore(container: Container, child: Node, before: Node) {
    container.children.splice(container.children.indexOf(before), 0, child);
  },
  removeChild(parent: Instance, child: Node) {
    parent.children.splice(parent.children.indexOf(child), 1);
  },
  removeChildFromContainer(container: Container, child: Node) {
    container.children.splice(container.children.indexOf(child), 1);
  },
  clearContainer(container: Container) {
    container.children = [];
  },

  finalizeInitialChildren: () => false,
  // Text inside a paragraph is rendered as ordinary children rather than set
  // as content, so an author's component keeps its hooks and its context all
  // the way down to the words.
  shouldSetTextContent: () => false,
  commitTextUpdate(instance: TextInstance, _old: string, text: string) {
    instance.text = text;
  },
  commitUpdate(instance: Instance, _type: string, _old: unknown, props: Record<string, unknown>) {
    instance.props = props;
  },

  // React insists on a host context object even when nothing varies by
  // position, so there is one, and it is the same one everywhere.
  getRootHostContext: () => HOST_CONTEXT,
  getChildHostContext: () => HOST_CONTEXT,
  getPublicInstance: (instance: Node) => instance,
  prepareForCommit: () => null,
  resetAfterCommit: () => {},
  preparePortalMount: () => {},
  detachDeletedInstance: () => {},
  // React 19 asks the host what priority an update runs at. There is one
  // render here and nothing to interrupt it, so it is always the default.
  resolveUpdatePriority: () => DefaultEventPriority,
  getCurrentUpdatePriority: () => currentPriority,
  setCurrentUpdatePriority: (priority: number) => {
    currentPriority = priority;
  },
  getInstanceFromNode: () => null,
  getInstanceFromScope: () => null,
  beforeActiveInstanceBlur: () => {},
  afterActiveInstanceBlur: () => {},
  prepareScopeUpdate: () => {},
  shouldAttemptEagerTransition: () => false,
  requestPostPaintCallback: () => {},
  maySuspendCommit: () => false,
  preloadInstance: () => true,
  startSuspendingCommit: () => {},
  suspendInstance: () => {},
  waitForCommitToBeReady: () => null,
  resetFormInstance: () => {},
  trackSchedulerEvent: () => {},
  resolveEventType: () => null,
  resolveEventTimeStamp: () => -1.1,
  NotPendingTransition: null,
  HostTransitionContext: {
    $$typeof: Symbol.for('react.context'),
    Provider: null,
    Consumer: null,
    _currentValue: null,
    _currentValue2: null,
    _threadCount: 0,
  },
};

/**
 * The two calls a one-shot render makes, with the shapes the runtime has.
 *
 * `react-reconciler`'s published types lag its own code: React 19 renamed the
 * priority hooks and gave `createContainer` two more error handlers. Declaring
 * what we actually call keeps the mismatch in one place and typed, instead of
 * a cast at each call site. What holds the config above honest is the tests —
 * every method there exists because leaving it out made one of them fail.
 */
interface Reconciler {
  createContainer(
    container: Container,
    tag: number,
    hydration: null,
    strict: boolean,
    concurrent: null,
    prefix: string,
    onUncaughtError: (error: unknown) => void,
    onCaughtError: (error: unknown) => void,
    onRecoverableError: (error: unknown) => void,
    transitions: null,
  ): object;
  updateContainer(element: unknown, root: object, parent: null, done: () => void): void;
}

// biome-ignore lint/suspicious/noExplicitAny: the published config type does not match the runtime's, which is the whole reason for `Reconciler` above.
type LooseConfig = any;

export const reconciler = ReactReconciler(config as LooseConfig) as unknown as Reconciler;
