import { useEffect, useRef, useState } from 'react';

/**
 * EditableName — a label that becomes an inline input on double-click (R20).
 * Commits on Enter/blur, cancels on Escape. Same pattern as the scene tabs.
 * Shared by NodeCard, HubNode and the virtual-device cards.
 *
 * `autoEdit` opens in edit mode straight away (e.g. naming a just-created
 * device). `placeholder` shows a hint when the value is empty.
 */
export function EditableName({
  value,
  onRename,
  className,
  autoEdit = false,
  placeholder,
}: {
  value: string;
  onRename?: (name: string) => void;
  className?: string;
  autoEdit?: boolean;
  placeholder?: string;
}) {
  const [editing, setEditing] = useState(autoEdit);
  const [draft, setDraft] = useState(value);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) {
      setDraft(value);
      ref.current?.focus();
      ref.current?.select();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);

  if (!editing) {
    return (
      <div
        className={`${className ?? ''} ${value ? '' : 'is-placeholder'}`}
        onDoubleClick={onRename ? () => setEditing(true) : undefined}
        title={onRename ? 'double-click to rename' : undefined}
      >
        {value || placeholder || ''}
      </div>
    );
  }

  const commit = () => {
    const name = draft.trim();
    if (name && name !== value) onRename?.(name);
    setEditing(false);
  };

  return (
    <input
      ref={ref}
      className={`${className ?? ''} name-input`}
      value={draft}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') setEditing(false);
      }}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    />
  );
}
