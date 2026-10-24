" vim's v:t_ type numbers (evalvars.c:131-165), including the ones Neovim lacks.
echo v:t_number v:t_string v:t_func v:t_list v:t_dict v:t_float v:t_bool v:t_none v:t_job v:t_channel v:t_blob v:t_class v:t_object v:t_typealias v:t_enum v:t_enumvalue v:t_tuple
echo type(v:null) == v:t_none
let v:t_none = 1
