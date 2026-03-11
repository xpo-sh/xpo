create table if not exists public.user_entitlements (
    user_id uuid primary key references auth.users (id) on delete cascade,
    plan text not null check (plan in ('free', 'pro', 'team')),
    max_tunnels integer not null check (max_tunnels > 0),
    max_ttl_secs bigint,
    allow_custom_subdomain boolean not null default false,
    updated_at timestamptz not null default now()
);

revoke all on public.user_entitlements from public;
revoke all on public.user_entitlements from anon;
revoke all on public.user_entitlements from authenticated;
grant select on public.user_entitlements to supabase_auth_admin;

create or replace function public.custom_access_token_hook(event jsonb)
returns jsonb
language plpgsql
stable
security definer
set search_path = public
as $$
declare
    claims jsonb;
    entitlement record;
begin
    select
        plan,
        max_tunnels,
        max_ttl_secs,
        allow_custom_subdomain
    into entitlement
    from public.user_entitlements
    where user_id = (event ->> 'user_id')::uuid;

    claims := coalesce(event -> 'claims', '{}'::jsonb);

    if not found then
        claims := jsonb_set(claims, '{xpo_plan}', to_jsonb('free'::text), true);
        claims := jsonb_set(claims, '{xpo_max_tunnels}', to_jsonb(1), true);
        claims := jsonb_set(claims, '{xpo_max_ttl_secs}', to_jsonb(3600), true);
        claims := jsonb_set(claims, '{xpo_allow_custom_subdomain}', to_jsonb(false), true);
    else
        claims := jsonb_set(claims, '{xpo_plan}', to_jsonb(entitlement.plan), true);
        claims := jsonb_set(claims, '{xpo_max_tunnels}', to_jsonb(entitlement.max_tunnels), true);
        if entitlement.max_ttl_secs is null then
            claims := claims - 'xpo_max_ttl_secs';
        else
            claims := jsonb_set(claims, '{xpo_max_ttl_secs}', to_jsonb(entitlement.max_ttl_secs), true);
        end if;
        claims := jsonb_set(
            claims,
            '{xpo_allow_custom_subdomain}',
            to_jsonb(entitlement.allow_custom_subdomain),
            true
        );
    end if;

    return jsonb_set(event, '{claims}', claims, true);
end;
$$;

grant execute on function public.custom_access_token_hook(jsonb) to supabase_auth_admin;
