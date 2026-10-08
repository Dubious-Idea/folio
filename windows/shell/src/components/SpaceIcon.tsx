import { Dynamic } from 'solid-js/web'
import { BriefcaseIcon, FolderIcon, LibraryIcon, UserIcon } from '../icons'

/** Значок пространства записан именем из SF Symbols (как на macOS); здесь он рисуется похожей иконкой. */
const ICONS: Record<string, typeof FolderIcon> = {
  briefcase: BriefcaseIcon,
  'briefcase.fill': BriefcaseIcon,
  person: UserIcon,
  'person.fill': UserIcon,
  'books.vertical': LibraryIcon,
  folder: FolderIcon,
}

export function SpaceIcon(props: { icon: string; size?: number }) {
  return <Dynamic component={ICONS[props.icon] ?? FolderIcon} size={props.size ?? 16} class="space-icon" />
}
