#ifndef KSUBSTRATE_H
#define KSUBSTRATE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t (*kh_i32_fn_t)(void);

int kh_hook_function(void *target, void *replacement, void **original);
/* expected_len must be >= 8 (the prologue patch window); shorter descriptions
 * are rejected so the whole overwritten region is always verified first. */
int kh_hook_function_checked(
    void *target,
    void *replacement,
    void **original,
    const void *expected_prologue,
    size_t expected_len
);
int kh_unhook_function(void *target);
/* PLT/GOT hook: replace an imported symbol's entry (preferred, update-stable).
 * Pass image=NULL to replace in all loaded images. */
int kh_hook_import(const char *image, const char *symbol, void *replacement, void **original);
void *kh_find_symbol(const char *image, const char *name);
/* Resolve a firmware-private function by module load base + RVA (from the
 * symbol DB) when it is not an exported symbol. Pair with kh_hook_function_checked
 * so a drifted RVA is refused by the prologue signature. */
void *kh_resolve_rva(const char *image, size_t rva);

int kh_register_named_i32_hook(const char *name, kh_i32_fn_t replacement);
int kh_clear_named_hook(const char *name);
int kh_call_named_i32(const char *name, kh_i32_fn_t original);

void kh_log(const char *message);

#define MSHookFunction kh_hook_function
#define MSFindSymbol kh_find_symbol

#ifdef __cplusplus
}
#endif

#endif
