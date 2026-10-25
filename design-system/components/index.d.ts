import type * as React from 'react';
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> { variant?: 'primary' | 'secondary' | 'quiet' }
export declare function Button(props: ButtonProps): React.ReactElement;
export interface StatusChipProps { tone?: 'final' | 'locked' | 'waiting' | 'refund' | 'danger'; children?: React.ReactNode }
export declare function StatusChip(props: StatusChipProps): React.ReactElement;
export interface AmountRowProps { label: string; value: string; unit: string; size?: 'large' | 'small' }
export declare function AmountRow(props: AmountRowProps): React.ReactElement;
export interface SwapSide { label?: string; value: string; unit: string }
export interface SwapCardProps { title?: string; status?: React.ReactNode; sell: SwapSide; receive: SwapSide; meta?: string; action?: string; onAction?: () => void; disabled?: boolean }
export declare function SwapCard(props: SwapCardProps): React.ReactElement;
export interface TimelineStep { label: string; detail?: string; time?: string; state?: 'done' | 'now' | 'todo' | 'failed' }
export interface TimelineProps { steps: TimelineStep[] }
export declare function Timeline(props: TimelineProps): React.ReactElement;
export interface NoticeProps { tone?: 'info' | 'warning' | 'success' | 'danger'; title?: string; children?: React.ReactNode }
export declare function Notice(props: NoticeProps): React.ReactElement;
export interface HashProps { value: string; label?: string; href?: string; head?: number; tail?: number }
export declare function Hash(props: HashProps): React.ReactElement;
export interface AmountInputProps { label: string; value: string; unit: string; onChange?: React.ChangeEventHandler<HTMLInputElement>; id?: string; placeholder?: string; hint?: string | null; error?: string | null }
export declare function AmountInput(props: AmountInputProps): React.ReactElement;
export interface PrivacyOption { value: string; label: string; detail: string }
export interface QuoteFormProps { sell: string; onSellChange?: React.ChangeEventHandler<HTMLInputElement>; sellUnit?: string; balance?: string; receive?: string; receiveUnit?: string; maker?: string; privacy: string; onPrivacyChange?: (value: string) => void; privacyOptions?: PrivacyOption[]; privacyNote?: string; action?: string; onSubmit?: () => void; disabled?: boolean; error?: string | null; id?: string }
export declare function QuoteForm(props: QuoteFormProps): React.ReactElement;
export interface InventoryItem { label: string; value: string; unit: string; note?: string }
export interface InventoryCardProps { title?: string; status?: React.ReactNode; items: InventoryItem[] }
export declare function InventoryCard(props: InventoryCardProps): React.ReactElement;
export interface SwapRowProps { id: string; sell: string; receive: string; step: string; deadline: string; tone?: StatusChipProps['tone']; statusLabel?: string }
export declare function SwapRow(props: SwapRowProps): React.ReactElement;
export interface LivenessAlarmProps { state?: 'upcoming' | 'open' | 'done'; title?: string; window?: string; countdown?: string; swapId?: string; children?: React.ReactNode }
export declare function LivenessAlarm(props: LivenessAlarmProps): React.ReactElement;
export interface LogoProps { size?: number; markOnly?: boolean }
export declare function Logo(props: LogoProps): React.ReactElement;
export interface IconProps { name: 'shield' | 'lock' | 'clock' | 'check' | 'return' | 'alert' | 'copy' | 'external' | 'arrow'; size?: number; title?: string; className?: string }
export declare function Icon(props: IconProps): React.ReactElement;
declare global { interface Window { Cloakroom: { Button: typeof Button; StatusChip: typeof StatusChip; AmountRow: typeof AmountRow; SwapCard: typeof SwapCard; Timeline: typeof Timeline; Notice: typeof Notice; Hash: typeof Hash; AmountInput: typeof AmountInput; QuoteForm: typeof QuoteForm; InventoryCard: typeof InventoryCard; SwapRow: typeof SwapRow; LivenessAlarm: typeof LivenessAlarm; Logo: typeof Logo; Icon: typeof Icon } } }
